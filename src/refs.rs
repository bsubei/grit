use sha1_smol::Digest;
use std::path::PathBuf;
use std::{fs, io::Write};

pub struct Refs {
    pathname: PathBuf,
}

// TODO the book creates a "Lockfile" to make sure two processes don't have race conditions reading the HEAD file and others. I'll leave this out until it's needed.
impl Refs {
    pub fn new(pathname: PathBuf) -> Self {
        Refs { pathname }
    }

    pub fn update_head(&mut self, oid: &Digest) {
        fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(self.get_head_path())
            .expect("failed to open HEAD to update")
            .write_all(oid.to_string().as_bytes())
            .expect("failed to write to HEAD");
    }

    pub fn read_head(&self) -> Option<Digest> {
        // Return None if no HEAD file exists, but panic if we fail to parse the digest in it.
        fs::read_to_string(self.get_head_path())
            .ok()
            .map(|contents| contents.trim().parse().ok())?
    }

    fn get_head_path(&self) -> PathBuf {
        self.pathname.join("HEAD")
    }
}
