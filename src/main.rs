use clap::Parser;
use git2::{FetchOptions, RemoteCallbacks, Repository};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

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

// Helper functions for consistent hashing
fn hash_string(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:.8x}", hasher.finalize()) // First 8 chars of hash
}

fn hash_git_url(url: &str) -> String {
    println!("ℹ️  Hashing git URL: {}", url);
    hash_string(url)
}

fn hash_git_option(option_type: &str, value: &str) -> String {
    println!("ℹ️  Hashing git option: {}-{}", option_type, value);
    hash_string(&format!("{}-{}", option_type, value))
}

/// Represents a parsed snippet filename
struct SnippetFile {
    prefix: String,
    commit_hash: String,
    full_name: String,
}

/// Functions to handle snippet file operations
impl SnippetFile {
    fn new(
        git_url: &str,
        git_option_type: &str,
        git_option_value: &str,
        path: &str,
        commit_sha: &str,
    ) -> Self {
        let path_buf = PathBuf::from(path);
        let file_name = path_buf
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("unknown");

        let prefix = format!(
            "{}-{}-{}-{}",
            hash_git_url(git_url),
            hash_git_option(git_option_type, git_option_value),
            hash_string(path),
            file_name,
        );

        let full_name = format!("{}-{}.rs", prefix, commit_sha);

        Self {
            prefix,
            commit_hash: commit_sha.to_string(),
            full_name,
        }
    }

    fn find_existing(prefix: &str) -> Option<Self> {
        let snippets_dir = std::path::Path::new(".snippets");
        if !snippets_dir.exists() {
            return None;
        }

        fs::read_dir(snippets_dir).ok()?.find_map(|entry| {
            let entry = entry.ok()?;
            let file_name = entry.file_name().to_string_lossy().to_string();

            if file_name.starts_with(prefix) {
                // Extract commit hash from filename
                let commit_hash = file_name
                    .strip_suffix(".rs")?
                    .rsplit('-')
                    .next()?
                    .to_string();

                Some(Self {
                    prefix: prefix.to_string(),
                    commit_hash,
                    full_name: file_name,
                })
            } else {
                None
            }
        })
    }
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

    // Determine git option type and value for hashing
    let (git_option_type, git_option_value, commit_sha) = if let Some(hash) = args.commit_hash {
        ("commit".to_string(), hash.clone(), hash)
    } else if let Some(tag) = &args.tag {
        (
            "tag".to_string(),
            tag.clone(),
            get_remote_commit_sha_without_clone(&args.git, None, Some(tag))?,
        )
    } else {
        // Handle branch case (including default branch)
        let default_branch = get_default_branch(&args.git)?;
        let branch_name = args
            .branch
            .as_deref()
            .unwrap_or(&default_branch)
            .to_string();
        (
            "branch".to_string(),
            branch_name.clone(),
            get_remote_commit_sha_without_clone(&args.git, Some(&branch_name), None)?,
        )
    };

    println!("✅ Found commit SHA: {}", commit_sha);
    println!("ℹ️  Git URL hash: {}", hash_git_url(&args.git));
    println!(
        "ℹ️  Git option hash: {}",
        hash_git_option(&git_option_type, &git_option_value)
    );
    println!("ℹ️  Path hash: {}", hash_string(&args.path));

    // Create new snippet file object
    let new_snippet = SnippetFile::new(
        &args.git,
        &git_option_type,
        &git_option_value,
        &args.path,
        &commit_sha,
    );

    println!("\n🔍 Checking for existing snippets...");

    // Check for existing snippet with same prefix
    if let Some(existing_snippet) = SnippetFile::find_existing(&new_snippet.prefix) {
        if existing_snippet.commit_hash == commit_sha {
            println!(
                "✅ Existing snippet is up to date at: .snippets/{}",
                existing_snippet.full_name
            );
            return Ok(());
        } else {
            println!("ℹ️  Found existing snippet with different commit hash:");
            println!("   Current: {}", existing_snippet.commit_hash);
            println!("   New: {}", commit_sha);
            println!("🔄 Updating snippet...");

            // Remove existing snippet
            fs::remove_file(Path::new(".snippets").join(&existing_snippet.full_name))?;
        }
    }

    // Create .snippets directory if it doesn't exist
    println!("\n📁 Creating .snippets directory if it doesn't exist...");
    std::fs::create_dir_all(".snippets")?;
    println!("✅ .snippets directory ready");

    // Clone repo and get content only if we need to update
    println!("\n📦 Cloning repository and checking out specific commit...");
    let temp_dir = clone_and_checkout_repo(
        &args.git,
        args.branch.as_deref(),
        args.tag.as_deref(),
        &commit_sha,
    )?;
    println!("✅ Repository cloned successfully");

    println!("\n📄 Reading source file...");
    let source_path = temp_dir.path().join(&args.path);
    let content = std::fs::read_to_string(&source_path)?;
    println!("✅ Successfully read file");

    println!("\n💾 Saving snippet...");
    let snippet_path = Path::new(".snippets").join(&new_snippet.full_name);
    std::fs::write(&snippet_path, content)?;
    println!("✅ Snippet saved successfully!");

    println!("\n📊 Summary:");
    println!("- Commit SHA: {}", commit_sha);
    println!("- Snippet saved to: .snippets/{}", new_snippet.full_name);

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

// fn hash_path(path: &str) -> String {
//     println!("ℹ️  Hashing path: {}", path);
//     let mut hasher = Sha256::new();
//     hasher.update(path.as_bytes());
//     format!("{:.8x}", hasher.finalize()) // First 8 chars of hash
// }

// Helper function to get default branch
fn get_default_branch(git_url: &str) -> Result<String, Box<dyn Error>> {
    let temp_dir = tempfile::Builder::new()
        .prefix("docify-temp-")
        .rand_bytes(5)
        .tempdir()?;

    let repo = Repository::init(temp_dir.path())?;
    let mut remote = repo.remote_anonymous(git_url)?;

    remote.connect(git2::Direction::Fetch)?;
    let default_branch = remote
        .default_branch()?
        .as_str()
        .ok_or("Invalid default branch name")?
        .to_string();
    remote.disconnect()?;

    Ok(default_branch
        .strip_prefix("refs/heads/")
        .unwrap_or(&default_branch)
        .to_string())
}
