mod blob;
mod commit;
mod database;
mod index;
mod object;
mod refs;
mod tree;
mod workspace;

use blob::Blob;
use commit::Commit;
use database::Database;
use index::Index;
use index::IndexMetadata;
use object::Object;
use refs::Refs;
use std::env;
use std::fs;
use std::io::{stdin, Result};
use std::path::PathBuf;
use std::time::SystemTime;
use tree::Tree;
use workspace::Workspace;

// TODO eventually take these in from a config or args.
const AUTHOR_NAME: &str = "bsubei";
const AUTHOR_EMAIL: &str = "6508762+bsubei@users.noreply.github.com";

fn main() -> Result<()> {
    // TODO use something like clap for arg parsing.
    let args: Vec<String> = env::args().collect();
    let subcommand = args.get(1).expect("missing subcommand");
    // TODO just assume root path is cwd. Will have to resolve this later.
    let root_path = env::current_dir().expect("failed to get cwd");

    let git_path = root_path.join(".git");
    let db_path = git_path.join("objects");
    let index_path = git_path.join("index");
    println!("path to root is: {:?}", root_path);
    println!("path to git is: {:?}", git_path);
    println!("path to db is: {:?}", db_path);

    match subcommand.as_str() {
        "init" => {
            fs::create_dir_all(git_path.join("objects")).expect("Could not create objects dir");
            fs::create_dir_all(git_path.join("refs")).expect("Could not create refs dir");
        }
        "commit" => {
            let ws = Workspace::new(root_path.clone());
            let mut database = Database::new(db_path);
            let mut refs = Refs::new(git_path.clone());
            let index = Index::new(index_path);

            // Store each file in the workspace as a Blob object on disk.
            // Also create FileEntry for each file.
            let files = index.get_filepaths();
            println!("Committing these files: {:?}", files);
            let entries: Vec<Blob> = files
                .into_iter()
                .map(|f| {
                    Blob::new(
                        ws.read_file(f).expect("Could not read file"),
                        f.to_path_buf(),
                    )
                })
                .collect();

            // Make a Tree object and store it on disk.
            let root_tree = Tree::new(entries);

            root_tree.traverse(&mut |subtree| {
                database.store(subtree);
            });

            let mut commit_message = String::new();
            stdin().read_line(&mut commit_message)?;

            let parent_ref = refs.read_head();
            let root_msg = match &parent_ref {
                Some(_) => "",
                _ => "(root-commit) ",
            };

            // Make a Commit object and write it to disk.
            let commit = Commit::new(
                *root_tree.get_oid(),
                parent_ref,
                AUTHOR_NAME.to_string(),
                AUTHOR_EMAIL.to_string(),
                SystemTime::now(),
                commit_message.to_string(),
            );
            database.store(&commit);

            // Update the "HEAD" file to "point" to the new commit.
            refs.update_head(commit.get_oid());

            let commit_hash = commit.get_oid();
            println!(
                "[{root_msg}{commit_hash} {}]",
                commit_message.lines().take(1).collect::<String>()
            );
        }
        // TODO we have to handle adding removed files (to support deleting files).
        "add" => {
            let input_filepaths = args
                .into_iter()
                .skip(2)
                .map(PathBuf::from)
                .collect::<Vec<_>>();

            let ws = Workspace::new(root_path);
            let mut database = Database::new(db_path);

            let mut index = Index::new(index_path);

            // TODO don't try to add/write files that already exist in the index unless they have changes.
            // For every user-given filepath, expand it (walk any directories), and add every resulting filepath.
            for input_filepath in input_filepaths {
                for expanded_filepath in ws.list_files(&input_filepath) {
                    let data = ws
                        .read_file(&expanded_filepath)
                        .expect("Could not read file in add");
                    let fs_metadata = ws
                        .stat_file(&expanded_filepath)
                        .expect("Could not get file metadata");

                    let blob = Blob::new(data, expanded_filepath.clone());
                    database.store(&blob);
                    index.add(
                        expanded_filepath,
                        *blob.get_oid(),
                        IndexMetadata::from(fs_metadata),
                    );
                }
            }
            index.write_updates();
        }
        _ => panic!("Unsupported subcommand: {}", subcommand),
    }
    Ok(())
}
