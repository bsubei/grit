use std::fmt::Debug;
use std::fmt::Formatter;
use std::{path::Path, path::PathBuf};

use crate::object::Object;
use is_executable::IsExecutable;
use sha1_smol::{Digest, Sha1};

// Technically, we only really care to sort by the path (there should never be two blobs with the same path, so the further sorting by oid and content won't matter).
#[derive(PartialOrd, PartialEq, Eq, Ord)]
pub struct Blob {
    path: PathBuf,
    oid: Digest,
    content: Vec<u8>,
}

impl Debug for Blob {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        fmt.debug_struct("Blob")
            .field("path", &self.path.display())
            .field("oid", &self.oid.to_string())
            .finish()
    }
}

impl Blob {
    pub fn new(bytes: Vec<u8>, path: PathBuf) -> Self {
        let content = format!("blob {}\0", bytes.len());
        let content = [content.as_bytes(), bytes.as_slice()].concat();
        let oid = Sha1::from(&content).digest();
        Blob { oid, content, path }
    }

    pub fn get_path(&self) -> &Path {
        &self.path
    }

    pub fn is_executable(&self) -> bool {
        self.path.is_executable()
    }
}

impl Object for Blob {
    fn get_oid(&self) -> &Digest {
        &self.oid
    }
    fn get_content(&self) -> &[u8] {
        &self.content
    }
}
