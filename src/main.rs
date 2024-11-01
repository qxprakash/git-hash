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
    commit_hash: Option<String>,

    #[arg(long)]
    path: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    // Validate that only one of branch, tag, or commit_hash is provided
    let options_count = [
        args.branch.is_some(),
        args.tag.is_some(),
        args.commit_hash.is_some(),
    ]
    .iter()
    .filter(|&&x| x)
    .count();

    if options_count > 1 {
        return Err("Only one of --branch, --tag, or --commit_hash can be specified".into());
    }

    println!("\n🔍 Fetching commit SHA from remote repository...");
    let commit_sha = if let Some(hash) = args.commit_hash {
        hash
    } else {
        get_remote_commit_sha_without_clone(&args.git, args.branch.as_deref(), args.tag.as_deref())?
    };
    println!("✅ Found commit SHA: {}", commit_sha);

    println!("\n📝 Generating snippet filename...");
    let filename = generate_snippet_filename(&commit_sha, &args.path);
    let snippet_path = std::path::Path::new(".snippets").join(&filename);
    println!("✅ Generated filename: {}", filename);

    println!("\n🔍 Checking if snippet already exists...");
    if snippet_path.exists() {
        println!("✅ Snippet already exists at: .snippets/{}", filename);
        println!("ℹ️  Skipping clone operation as file is already present");
        return Ok(());
    }
    println!("ℹ️  Snippet not found, proceeding with clone operation");

    println!("\n📦 Cloning repository and checking out specific commit...");
    let temp_dir = clone_and_checkout_repo(
        &args.git,
        args.branch.as_deref(),
        args.tag.as_deref(),
        &commit_sha,
    )?;
    println!(
        "✅ Repository cloned successfully at: {}",
        temp_dir.path().display()
    );

    println!("\n📁 Creating .snippets directory if it doesn't exist...");
    std::fs::create_dir_all(".snippets")?;
    println!("✅ .snippets directory ready");

    println!("\n📄 Reading source file...");
    let source_path = temp_dir.path().join(&args.path);
    let content = std::fs::read_to_string(&source_path)?;
    println!("✅ Successfully read file: {}", source_path.display());

    println!("\n💾 Saving snippet...");
    std::fs::write(&snippet_path, content)?;
    println!("✅ Snippet saved successfully!");

    println!("\n📊 Summary:");
    println!("- Commit SHA: {}", commit_sha);
    println!("- Path hash: {}", hash_path(&args.path));
    println!("- Repo location: {}", temp_dir.path().display());
    println!("- Snippet saved to: .snippets/{}", filename);

    // Prevent temp_dir from being deleted
    std::mem::forget(temp_dir);
    println!("\n✨ Operation completed successfully!");

    Ok(())
}

fn get_remote_commit_sha_without_clone(
    git_url: &str,
    branch: Option<&str>,
    tag: Option<&str>,
) -> Result<String, Box<dyn Error>> {
    let temp_dir = tempfile::Builder::new()
        .prefix("docify-temp-")
        .rand_bytes(5)
        .tempdir()?;

    let repo = Repository::init(temp_dir.path())?;
    let mut remote = repo.remote_anonymous(git_url)?;

    // First, fetch the remote HEAD to determine default branch
    println!("ℹ️  Fetching remote references...");
    remote.connect(git2::Direction::Fetch)?;
    let default_branch = remote
        .default_branch()?
        .as_str()
        .ok_or("Invalid default branch name")?
        .to_string();
    remote.disconnect()?;

    // Convert refs/heads/main to just main
    let default_branch = default_branch
        .strip_prefix("refs/heads/")
        .unwrap_or(&default_branch);

    println!("ℹ️  Default branch: {}", default_branch);

    // Determine which refs to fetch
    let refspecs = if let Some(tag_name) = tag {
        vec![format!("refs/tags/{}:refs/tags/{}", tag_name, tag_name)]
    } else {
        let branch_name = branch.unwrap_or(default_branch);
        vec![format!(
            "refs/heads/{}:refs/heads/{}",
            branch_name, branch_name
        )]
    };

    println!("ℹ️  Refspecs: {:?}", refspecs);

    // Fetch the required refs
    println!("ℹ️  Fetching required references...");
    remote.fetch(
        refspecs
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .as_slice(),
        None,
        None,
    )?;

    // Get the commit ID
    let commit_id = if let Some(tag_name) = tag {
        let tag_ref = repo.find_reference(&format!("refs/tags/{}", tag_name))?;
        tag_ref.peel_to_commit()?.id()
    } else {
        let branch_name = branch.unwrap_or(default_branch);
        let reference = repo.find_reference(&format!("refs/heads/{}", branch_name))?;
        reference.peel_to_commit()?.id()
    };

    Ok(commit_id.to_string())
}

fn clone_and_checkout_repo(
    git_url: &str,
    _branch: Option<&str>,
    _tag: Option<&str>,
    commit_sha: &str,
) -> Result<tempfile::TempDir, Box<dyn Error>> {
    let temp_dir = tempfile::Builder::new()
        .prefix("docify-temp-")
        .rand_bytes(5)
        .tempdir()?;

    let repo = Repository::init(temp_dir.path())?;

    let mut callbacks = RemoteCallbacks::new();
    callbacks.transfer_progress(|p| {
        println!(
            "📥 Fetching: {}/{} objects ({:.1}%)",
            p.received_objects(),
            p.total_objects(),
            (p.received_objects() as f64 / p.total_objects() as f64) * 100.0
        );
        true
    });

    let mut fetch_opts = FetchOptions::new();
    fetch_opts.remote_callbacks(callbacks);
    fetch_opts.depth(1);

    let mut remote = repo.remote_anonymous(git_url)?;

    // Only fetch the specific commit we need
    remote.fetch(
        &[&format!("+{commit_sha}:refs/heads/temp")],
        Some(&mut fetch_opts),
        None,
    )?;

    // Checkout the specific commit
    let commit_id = git2::Oid::from_str(commit_sha)?;
    let commit = repo.find_commit(commit_id)?;
    let tree = commit.tree()?;
    repo.checkout_tree(tree.as_object(), None)?;
    repo.set_head_detached(commit_id)?;

    Ok(temp_dir)
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
