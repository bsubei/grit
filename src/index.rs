use sha1_smol::{Digest, Sha1};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, File},
    io::{BufRead, Cursor, Read, Write},
    os::{linux::fs::MetadataExt, unix::fs::PermissionsExt},
    path::{Path, PathBuf},
};

use crate::tree::Tree;

const REGULAR_MODE: u32 = 0o100644;
const EXECUTABLE_MODE: u32 = 0o100755;
const MAX_PATH_SIZE: u32 = 0xfff;

const SIGNATURE: &[u8] = b"DIRC";
const VERSION: u32 = 2;

#[derive(Debug, Default, PartialEq)]
pub struct IndexMetadata {
    ctime: u32,
    ctime_nsec: u32,
    mtime: u32,
    mtime_nsec: u32,
    dev: u32,
    ino: u32,
    mode: u32,
    uid: u32,
    gid: u32,
    size: u32,
}

impl From<fs::Metadata> for IndexMetadata {
    fn from(m: fs::Metadata) -> Self {
        // NOTE: I extracted this directly from the is_executable crate.
        let mode = if m.permissions().mode() & 0o111 != 0 {
            EXECUTABLE_MODE
        } else {
            REGULAR_MODE
        };

        IndexMetadata {
            ctime: m.st_ctime() as u32,
            ctime_nsec: m.st_ctime_nsec() as u32,
            mtime: m.st_mtime() as u32,
            mtime_nsec: m.st_mtime_nsec() as u32,
            dev: m.st_dev() as u32,
            ino: m.st_ino() as u32,
            mode,
            uid: m.st_uid(),
            gid: m.st_gid(),
            size: m.st_size() as u32,
        }
    }
}

impl From<[u8; 40]> for IndexMetadata {
    fn from(input: [u8; 40]) -> Self {
        let mut buf = [0; 4];
        let mut cursor = Cursor::new(input);

        let fields: Vec<u32> = (0..10)
            .map(|_| {
                cursor
                    .read_exact(&mut buf)
                    .expect("Failed to read entry field");
                u32::from_be_bytes(buf)
            })
            .collect();

        IndexMetadata {
            ctime: fields[0],
            ctime_nsec: fields[1],
            mtime: fields[2],
            mtime_nsec: fields[3],
            dev: fields[4],
            ino: fields[5],
            mode: fields[6],
            uid: fields[7],
            gid: fields[8],
            size: fields[9],
        }
    }
}

#[derive(Debug, PartialEq)]
struct IndexEntry {
    path: PathBuf,
    oid: Digest,
    metadata: IndexMetadata,
}

const ENTRY_BLOCK: usize = 8;
impl IndexEntry {
    fn to_data(&self) -> Vec<u8> {
        // NOTE: each index entry is serialized using the format "N10H40nZ*" as follows:
        // - Ten 32-bit unsigned big-endian numbers (ctime sec, ctime nsec, mtime sec, mtime nsec, dev, ino, mode, uid, gid, size).
        // - the SHA (oid), which will be packed as 20 bytes
        // - a 16-bit unsigned big-endian number (flags)
        // - a variable-length null-terminated string. This string is padded with null bytes to a multiple of 8 (block size).

        // Pack the entry fields as above.
        let fields: Vec<u32> = vec![
            self.metadata.ctime,
            self.metadata.ctime_nsec,
            self.metadata.mtime,
            self.metadata.mtime_nsec,
            self.metadata.dev,
            self.metadata.ino,
            self.metadata.mode,
            self.metadata.uid,
            self.metadata.gid,
            self.metadata.size,
        ];
        let mut v = fields
            .into_iter()
            .flat_map(|num| num.to_be_bytes())
            .collect::<Vec<_>>();
        v.extend_from_slice(&self.oid.bytes());
        // NOTE: flags will only have the byte size and it has to fit in 16 bits.
        let path = self.path.to_string_lossy();
        let flags = path.len().min(MAX_PATH_SIZE as usize) as u16;
        v.extend_from_slice(&flags.to_be_bytes());
        v.extend_from_slice(path.as_bytes());

        // Now keep padding with zeros until we reach a multiple of the block size.
        let remaining = ENTRY_BLOCK as i32 - ((v.len() % ENTRY_BLOCK) as i32);
        v.extend_from_slice(&vec![0; remaining as usize]);

        v
    }
    fn read_entry<T>(data: &mut Cursor<T>) -> Self
    where
        T: AsRef<[u8]>,
    {
        let start = data.position();
        // Create a Cursor that you read from, so it moves the position automatically.
        let mut fields = [0; 40];
        data.read_exact(&mut fields)
            .expect("Failed to read entry fields");
        let metadata = IndexMetadata::from(fields);

        let mut sha = [0; 20];
        data.read_exact(&mut sha).expect("Failed to read entry sha");
        let oid = Sha1::from(sha).digest();

        let mut flags = [0; 2];
        data.read_exact(&mut flags)
            .expect("Failed to read entry flags");
        let length = u16::from_be_bytes(flags) as usize;

        // In order to read the path, either use the given length, or keep reading until we hit a null char.
        let path = if length < MAX_PATH_SIZE as usize {
            let mut buf = vec![0; length];
            data.read_exact(&mut buf)
                .expect("Failed to read entry path");

            // Because the other match clause consumes one null char, we must do the same here.
            let mut null = [0; 1];
            data.read_exact(&mut null)
                .expect("Failed to read null char");
            assert!(null[0] == 0);

            String::from_utf8(buf).expect("Entry path not UTF-8")
        } else {
            // Read null-terminated string.

            let buf = data
                .bytes()
                .map_while(|b| {
                    let byte = b.expect("Failed to read unknown-length null-terminated entry path");
                    (byte != 0).then_some(byte)
                })
                .collect::<Vec<_>>();
            // NOTE: that map_while on a consuming iterator consumes the last element (the null char where it stops).
            String::from_utf8(buf).expect("Entry path not UTF-8")
        };
        let end = data.position();
        let num_bytes_consumed = (end - start) as usize;
        // Remove any padding up to ENTRY_BLOCK. Take into account that we've already removed one null char byte.
        let remaining =
            (ENTRY_BLOCK as i32 - ((num_bytes_consumed % ENTRY_BLOCK) as i32)) % ENTRY_BLOCK as i32;
        let mut padding = vec![0; remaining as usize];
        data.read_exact(&mut padding)
            .expect("Failed to read entry padding");
        assert!(padding.iter().all(|b| *b == 0));

        let path = PathBuf::from(path);
        IndexEntry {
            path,
            oid,
            metadata,
        }
    }
}

#[derive(Debug, Default)]
pub struct Index {
    path: PathBuf,
    entries: BTreeMap<PathBuf, IndexEntry>,
    // This "parents_to_children" field maps each directory to all the paths (files) that it is a parent of. It's fully derived from "entries" and is used
    // as a faster way to access a given directory's children (e.g. remove_children).
    parents_to_children: HashMap<PathBuf, HashSet<PathBuf>>,
}

impl Index {
    pub fn new(path: PathBuf) -> Self {
        // Read given index path file (if it exists) and fill up the entries with what it contains.

        match std::fs::read(&path) {
            Err(_) => Index {
                path,
                ..Default::default()
            },
            Ok(buf) => {
                let mut cursor = Cursor::new(buf);
                let length = Self::read_header(&mut cursor);
                let entries: BTreeMap<_, _> = (0..length)
                    .map(|_| {
                        let entry = IndexEntry::read_entry(&mut cursor);
                        (entry.path.clone(), entry)
                    })
                    .collect();

                // Read the last 20 bytes from the index file and compare them to the SHA formed by the rest of the file.
                let mut sha = [0; 20];
                cursor
                    .read_exact(&mut sha)
                    .expect("Failed to read SHA from index file");

                // Without copying the original buf/cursor, read all the bytes except the last 20, and make sure their hash matches the sha we read.
                let num_bytes_so_far = cursor.position();
                cursor.set_position(0);
                let mut x = cursor.take(num_bytes_so_far - 20);
                let bytes_to_hash = x.fill_buf().unwrap();
                assert!(sha == Sha1::from(bytes_to_hash).digest().bytes());

                // TODO consider only constructing this when it's needed (maybe using interior mutability to populate it behind the scenes when it's first needed and then reusing it on subsequent calls)
                // Construct the parents_to_children "cache" so we can easily find the children of any given dir entry.
                let parents_to_children = Self::construct_parents_cache(&entries);

                Index {
                    path,
                    entries,
                    parents_to_children,
                }
            }
        }
    }

    fn construct_parents_cache(
        entries: &BTreeMap<PathBuf, IndexEntry>,
    ) -> HashMap<PathBuf, HashSet<PathBuf>> {
        let mut parents_to_children: HashMap<PathBuf, HashSet<PathBuf>> = HashMap::new();
        for entry_path in entries.keys() {
            for parent_dir in Tree::get_parent_directories(entry_path) {
                match parents_to_children.get_mut(&parent_dir) {
                    Some(children) => {
                        children.insert(entry_path.clone());
                    }

                    None => {
                        let mut children = HashSet::new();
                        children.insert(entry_path.clone());
                        parents_to_children.insert(parent_dir, children);
                    }
                }
            }
        }
        parents_to_children
    }

    pub fn get_filepaths(&self) -> Vec<&PathBuf> {
        self.entries.keys().collect()
    }

    fn discard_conflicts(&mut self, conflicting_path: &Path) {
        // If an existing entry conflicts with this new one, remove the old entry.
        // This handles the case when the existing entry is just a file.
        for parent_dir in Tree::get_parent_directories(conflicting_path) {
            self.remove_entry(&parent_dir);
        }
        self.remove_children(conflicting_path);
    }

    fn remove_children(&mut self, path: &Path) {
        // This handles the case when the existing entry is a directory (and we have to recursively remove its entries).
        if let Some(children) = self.parents_to_children.get(path) {
            println!("removing children: {}", path.display());
            for child in children.clone() {
                self.remove_entry(&child);
            }
        };
    }

    fn remove_entry(&mut self, path: &Path) {
        // If such an entry exists,
        if let Some(entry) = self.entries.get(path) {
            println!("removing entry: {}", path.display());
            let entry_path = entry.path.clone();
            // Remove the entry from entries.
            self.entries.remove(&entry_path);
            // Also remove the entry from the parents_to_children field. That means go over the parent dirs of this entry,
            // and for each such parent dir, remove its children. Finally, remove the parent dir itself.
            for parent in Tree::get_parent_directories(&entry_path) {
                if let Some(children) = self.parents_to_children.get_mut(&parent) {
                    if children.is_empty() {
                        self.parents_to_children.remove(&parent);
                    } else {
                        children.remove(&entry_path);
                    }
                }
            }
        };
    }

    pub fn add(&mut self, path: PathBuf, oid: Digest, metadata: IndexMetadata) {
        println!("Adding {} to index!", path.display());
        self.discard_conflicts(&path);

        self.store_entry(IndexEntry {
            path,
            oid,
            metadata,
        });
    }

    fn store_entry(&mut self, entry: IndexEntry) {
        let entry_path = entry.path.clone();

        self.entries.insert(entry_path.clone(), entry);

        // TODO this whole block is repeated in construct_parents_cache(). Refactor it out by making a similar func to populate the parents_to_children for a single entry.
        // Now populate the parents_to_children for this new entry.
        for parent_dir in Tree::get_parent_directories(&entry_path) {
            match self.parents_to_children.get_mut(&parent_dir) {
                Some(children) => {
                    children.insert(entry_path.clone());
                }
                None => {
                    let mut children = HashSet::new();
                    children.insert(entry_path.clone());
                    self.parents_to_children.insert(parent_dir, children);
                }
            }
        }
    }

    pub fn write_updates(&mut self) {
        // TODO the book author decides to write out the index incrementally (entry by entry) and then finish (this allows for also building the SHA digest incrementally).
        // We shall dispense with such fanciness.
        let mut data = self.get_header();
        data.append(
            &mut self
                .entries
                .values()
                .flat_map(|entry| entry.to_data())
                .collect::<Vec<_>>(),
        );
        let sha = Sha1::from(&data);
        data.append(&mut sha.digest().bytes().into());

        let mut f = File::create(&self.path).expect("Could not open index file");
        f.write_all(&data)
            .expect("Could not write_all to index file");
    }

    fn get_header(&self) -> Vec<u8> {
        // NOTE: we're trying to replicate the byte packing of "a4N2", which packs a 4-byte string followed by two 32-bit big-endian numbers.
        let mut v = Vec::with_capacity(12);
        v.extend_from_slice(b"DIRC");
        v.extend_from_slice(&2_u32.to_be_bytes());
        // NOTE: "as u32" will truncate the entries len. i.e. we can't have more than (2^32 -1) index entries length.
        v.extend_from_slice(&(self.entries.len() as u32).to_be_bytes());
        v
    }

    fn read_header<T>(buf: &mut Cursor<T>) -> u32
    where
        T: AsRef<[u8]>,
    {
        let mut signature = [0; 4];
        buf.read_exact(&mut signature)
            .expect("Failed to read index header signature");
        assert!(signature == SIGNATURE);

        let mut version = [0; 4];
        buf.read_exact(&mut version)
            .expect("Failed to read index header version");
        assert!(u32::from_be_bytes(version) == VERSION);

        let mut length = [0; 4];
        buf.read_exact(&mut length)
            .expect("Failed to read index header length");
        u32::from_be_bytes(length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_index() -> Index {
        const INDEX_PATH: &str = "some_index";
        Index {
            path: PathBuf::from(INDEX_PATH),
            ..Default::default()
        }
    }
    #[test]
    fn test_add_basic() {
        let mut index = empty_index();
        let filepath = PathBuf::from("filepath");
        let fake_digest = Sha1::from("").digest();

        index.add(filepath.clone(), fake_digest, IndexMetadata::default());
        let expected_entry = IndexEntry {
            path: filepath.clone(),
            oid: fake_digest,
            metadata: IndexMetadata::default(),
        };
        assert_eq!(index.entries.len(), 1);
        assert_eq!(*index.entries.get(&filepath).unwrap(), expected_entry);
    }

    #[test]
    fn test_add_discard_conflicts_file() {
        let mut index = empty_index();
        let filepaths = ["alice.txt", "bob.txt", "alice.txt/nested.txt"]
            .iter()
            .map(|path| PathBuf::from(path))
            .collect::<Vec<_>>();
        let fake_digest = Sha1::from("").digest();

        for filepath in filepaths {
            index.add(filepath.clone(), fake_digest, IndexMetadata::default());
        }
        // There should be only two entries, because "alice.txt" was conflicting with the last one and was removed.
        // Also, the entries are ordered alphabetically.
        assert_eq!(index.entries.len(), 2);
        let expected_filepaths = ["alice.txt/nested.txt", "bob.txt"];
        for (entry_key, expected_filepath) in index.entries.keys().zip(expected_filepaths) {
            assert_eq!(entry_key.to_string_lossy(), expected_filepath);
        }
    }

    #[test]
    fn test_add_discard_conflicts_dir() {
        let mut index = empty_index();
        let filepaths = [
            "alice.txt",
            "nested/bob.txt",
            "nested/inner/claire.txt",
            "nested",
        ]
        .iter()
        .map(|path| PathBuf::from(path))
        .collect::<Vec<_>>();
        let fake_digest = Sha1::from("").digest();

        for filepath in filepaths {
            index.add(filepath.clone(), fake_digest, IndexMetadata::default());
        }
        // There should be only two entries, because everything in the "nested/" dir was conflicting with the "nested" file we added most recently.
        // Also, the entries are ordered alphabetically.
        assert_eq!(index.entries.len(), 2);
        let expected_filepaths = ["alice.txt", "nested"];
        for (entry_key, expected_filepath) in index.entries.keys().zip(expected_filepaths) {
            assert_eq!(entry_key.to_string_lossy(), expected_filepath);
        }
    }
}
