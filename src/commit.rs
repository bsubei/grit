use std::time::SystemTime;

use crate::object::Object;
use sha1_smol::{Digest, Sha1};

pub struct Commit {
    oid: Digest,
    content: Vec<u8>,
}

impl Commit {
    pub fn new(
        tree_oid: Digest,
        parent: Option<Digest>,
        author_name: String,
        author_email: String,
        timestamp: SystemTime,
        commit_message: String,
    ) -> Self {
        // The data that the commit stores/points-to is the tree, author, committer, and the commit message.
        let mut data = String::new();

        data.push_str(&format!("tree {}\n", tree_oid));

        if let Some(parent) = parent {
            data.push_str(&format!("parent {}\n", parent));
        }

        let seconds = timestamp
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("Could not get system time to record in commit")
            .as_secs();
        // TODO deal with timezones eventually, hardcoded mine (NYC) for now.
        let timezone = "-0400";
        let formatted_timestamp = format!("{seconds} {timezone}");
        // TODO for now, commiter name/email are the same as author name/email.
        data.push_str(&format!(
            "author {author_name} <{author_email}> {formatted_timestamp}\n"
        ));
        data.push_str(&format!(
            "committer {author_name} <{author_email}> {formatted_timestamp}\n\n"
        ));

        data.push_str(&commit_message);

        // The commit content consists of this commit prefix + the actual "data".
        let content = format!("commit {}\0{}", data.len(), data).into_bytes();

        // The oid for this commit is the Sha of its content.
        let oid = Sha1::from(&content).digest();
        Commit { oid, content }
    }
}

impl Object for Commit {
    fn get_oid(&self) -> &Digest {
        &self.oid
    }
    fn get_content(&self) -> &[u8] {
        &self.content
    }
}
