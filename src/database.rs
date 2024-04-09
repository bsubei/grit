use crate::object::Object;
use flate2::{write::ZlibEncoder, Compression};
use sha1_smol::Digest;
use std::fs::File;

use std::{fs, io::ErrorKind, io::Write, path::PathBuf};

pub struct Database {
    path: PathBuf,
}

impl Database {
    pub fn new(path: PathBuf) -> Self {
        Database { path }
    }

    pub fn store<T>(&mut self, object: &T)
    where
        T: Object + ?Sized,
    {
        self.write_object(object.get_oid(), object.get_content());
    }

    fn write_object(&self, oid: &Digest, content: &[u8]) {
        let oid = oid.to_string();
        let object_path = self.path.join(&oid[0..2]).join(&oid[2..]);
        let dirname = object_path
            .parent()
            .expect("Cannot get parent dir for object");

        fs::create_dir_all(dirname).expect("Could not create dir to write object");
        let mut f = match File::options()
            .create_new(true)
            .write(true)
            .open(object_path)
        {
            Ok(f) => f,
            // Return early if this object already exists, no use writing it again.
            Err(e) if e.kind() == ErrorKind::AlreadyExists => return,
            // Fail loudly if the object doesn't already exist but we can't open the file.
            _ => panic!("failed to open object file to write"),
        };

        // The book says BEST_SPEED and that seems to match with this library's "fast" mode.
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder
            .write_all(content)
            .expect("Could not deflate encode object");
        f.write_all(&encoder.finish().expect("Could not flush deflate encode"))
            .expect("Could not write encoded data to blob file");
    }
}
