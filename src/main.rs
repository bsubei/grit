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
use std::io;
use std::io::stdin;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tree::Tree;
use workspace::Workspace;

// TODO eventually take these in from a config or args.
const AUTHOR_NAME: &str = "bsubei";
const AUTHOR_EMAIL: &str = "6508762+bsubei@users.noreply.github.com";

fn main() -> io::Result<()> {
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

            // TODO currently, we can't read HEAD files that refer to branches (we assume hashes only). That means we can't test on this repo because we used git to create branches.
            let parent_ref = refs.read_head();
            let root_msg = match &parent_ref {
                Some(_) => "",
                _ => "(root-commit) ",
            };

            // TODO only go ahead and create a commit if there is something to commit. Likely have to compare the commit's root tree hash with the parent's tree hash.
            // TODO FIX BUG where add is adding unrelated files to the index. Might be related to above.
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
            let mut input_filepaths = args
                .into_iter()
                .skip(2)
                .map(PathBuf::from)
                .collect::<Vec<_>>();
            // Default to adding the root path.
            if input_filepaths.is_empty() {
                input_filepaths.push(root_path.clone());
            }

            let ws = Workspace::new(root_path);
            let mut database = Database::new(db_path);

            let mut index = Index::new(index_path);

            // TODO don't try to add/write files that already exist in the index unless they have changes.
            // For every user-given filepath, expand it (walk any directories), and add every resulting filepath.
            let expanded_filepaths: walkdir::Result<Vec<PathBuf>> = input_filepaths
                .iter()
                .map(|input_filepath| ws.list_files(input_filepath))
                .collect::<walkdir::Result<Vec<_>>>()
                .map(|t| t.concat());

            match expanded_filepaths {
                Err(e) => {
                    let path = e.path().unwrap_or(Path::new(""));
                    if let Some(error) = e.io_error() {
                        match error.kind() {
                            io::ErrorKind::NotFound => {
                                eprintln!(
                                    "fatal: pathspec '{}' did not match any files",
                                    path.display()
                                );
                            }
                            _ => {
                                eprintln!(
                                    "fatal: pathspec {} had an unknown io error",
                                    path.display()
                                );
                            }
                        }
                    } else {
                        eprintln!("fatal: pathspec {} had an unknown error", path.display());
                    }
                    std::process::exit(128);
                }
                Ok(expanded_filepaths) => {
                    for expanded_filepath in expanded_filepaths {
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
            };

            index.write_updates();
        }
        _ => panic!("Unsupported subcommand: {}", subcommand),
    }
    Ok(())
}
