use clap::Parser;
use git2::{FetchOptions, RemoteCallbacks, Repository};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long)]
    git: String,

    #[arg(long)]
    branch: Option<String>,

    #[arg(long)]
    tag: Option<String>,

    #[arg(long)]
    path: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    if args.branch.is_some() && args.tag.is_some() {
        return Err("Cannot specify both branch and tag".into());
    }

    let (commit_sha, temp_dir) =
        get_remote_commit_sha(&args.git, args.branch.as_deref(), args.tag.as_deref())?;
    let filename = generate_snippet_filename(&commit_sha, &args.path);

    println!("Generated filename: {}", filename);
    println!("Commit SHA: {}", commit_sha);
    println!("Path hash: {}", hash_path(&args.path));
    println!("Repo cloned at: {}", temp_dir.path().display());

    // Prevent temp_dir from being deleted by forgetting it
    std::mem::forget(temp_dir);

    Ok(())
}

fn get_remote_commit_sha(
    git_url: &str,
    branch: Option<&str>,
    tag: Option<&str>,
) -> Result<(String, tempfile::TempDir), Box<dyn Error>> {
    let temp_dir = tempfile::Builder::new()
        .prefix("docify-temp-")
        .rand_bytes(5)
        .tempdir()?;

    let repo = Repository::init(temp_dir.path())?;

    let mut callbacks = RemoteCallbacks::new();
    callbacks.transfer_progress(|p| {
        println!(
            "Fetching: {}/{} objects",
            p.received_objects(),
            p.total_objects()
        );
        true
    });

    let mut fetch_opts = FetchOptions::new();
    fetch_opts.remote_callbacks(callbacks);

    let mut remote = repo.remote_anonymous(git_url)?;

    // Fetch all refs
    remote.fetch(&["refs/*:refs/*"], Some(&mut fetch_opts), None)?;

    let commit_id = if let Some(tag_name) = tag {
        let tag_ref = repo.find_reference(&format!("refs/tags/{}", tag_name))?;
        tag_ref.peel_to_commit()?.id()
    } else {
        let branch_name = branch.unwrap_or("master");
        // Look for the reference in different possible locations
        let reference = repo
            .find_reference(&format!("refs/remotes/origin/{}", branch_name))
            .or_else(|_| repo.find_reference(&format!("refs/heads/{}", branch_name)))
            .or_else(|_| repo.find_reference(&format!("refs/{}", branch_name)))?;

        reference.peel_to_commit()?.id()
    };

    Ok((commit_id.to_string(), temp_dir))
}

fn hash_path(path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());
    format!("{:.8x}", hasher.finalize()) // First 8 chars of hash
}

fn generate_snippet_filename(commit_sha: &str, path: &str) -> String {
    let path_buf = PathBuf::from(path);
    let file_name = path_buf
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("unknown");

    format!(
        "{}-{}-{}",
        &commit_sha[..8], // First 8 chars of commit SHA
        hash_path(path),  // Hash of full path
        file_name         // Original filename
    )
}
