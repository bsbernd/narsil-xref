//! Repository management utilities
//!
//! Handles repository discovery, validation, and configuration.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Configuration for a repository
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    /// Repository name
    pub name: String,

    /// Path to repository root
    pub path: PathBuf,

    /// Patterns to exclude from indexing
    pub exclude_patterns: Vec<String>,

    /// Patterns to include (if empty, include all)
    pub include_patterns: Vec<String>,

    /// Maximum file size to index (bytes)
    pub max_file_size: u64,

    /// Whether to follow symlinks
    pub follow_symlinks: bool,
}

impl Default for RepoConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            path: PathBuf::new(),
            exclude_patterns: vec![
                "**/node_modules/**".to_string(),
                "**/target/**".to_string(),
                "**/.git/**".to_string(),
                "**/vendor/**".to_string(),
                "**/__pycache__/**".to_string(),
                "**/dist/**".to_string(),
                "**/build/**".to_string(),
                "**/*.min.js".to_string(),
                "**/*.min.css".to_string(),
                "**/package-lock.json".to_string(),
                "**/yarn.lock".to_string(),
                "**/Cargo.lock".to_string(),
            ],
            include_patterns: vec![],
            max_file_size: 1024 * 1024, // 1MB
            follow_symlinks: false,
        }
    }
}

/// Discover repositories in a directory
pub fn discover_repos(base_path: &Path, max_depth: usize) -> Result<Vec<PathBuf>> {
    let mut repos = Vec::new();
    discover_repos_recursive(base_path, 0, max_depth, &mut repos)?;
    Ok(repos)
}

fn discover_repos_recursive(
    path: &Path,
    depth: usize,
    max_depth: usize,
    repos: &mut Vec<PathBuf>,
) -> Result<()> {
    if depth > max_depth {
        return Ok(());
    }

    // Check if this directory is a repository
    if is_repository(path) {
        repos.push(path.to_path_buf());
        return Ok(()); // Don't recurse into repos
    }

    // Recurse into subdirectories
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let entry_path = entry.path();

            if entry_path.is_dir() {
                let name = entry_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");

                // Skip hidden directories
                if !name.starts_with('.') {
                    discover_repos_recursive(&entry_path, depth + 1, max_depth, repos)?;
                }
            }
        }
    }

    Ok(())
}

/// Check if a directory is a repository root
pub fn is_repository(path: &Path) -> bool {
    // Check for common VCS directories
    if path.join(".git").exists() {
        return true;
    }

    // Check for common project files
    let project_markers = [
        "Cargo.toml",     // Rust
        "package.json",   // Node.js
        "pyproject.toml", // Python
        "setup.py",       // Python
        "go.mod",         // Go
        "pom.xml",        // Java/Maven
        "build.gradle",   // Java/Gradle
        "CMakeLists.txt", // C/C++
        "Makefile",       // Generic
        ".project",       // Eclipse
        "*.sln",          // .NET
    ];

    for marker in &project_markers {
        if marker.contains('*') {
            // Glob pattern
            if let Ok(entries) = glob::glob(&path.join(marker).to_string_lossy()) {
                if entries.filter_map(|e| e.ok()).count() > 0 {
                    return true;
                }
            }
        } else if path.join(marker).exists() {
            return true;
        }
    }

    false
}

/// Get the repository name from a path
pub fn repo_name_from_path(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}

/// The main checkout's root `path` is a linked worktree of, or `None` when
/// `path` is not a linked worktree (a plain checkout, not a git repo at
/// all, or a `git` too old for `--path-format`).
///
/// A plain checkout's `git-common-dir` is its own `.git`; a linked
/// worktree's points at the main checkout's `.git` instead, which is what
/// distinguishes the two here.
pub fn worktree_main_repo(path: &Path) -> Option<PathBuf> {
    let canonical_path = path.canonicalize().ok()?;
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(&canonical_path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let common_dir = String::from_utf8(output.stdout).ok()?;
    let root = PathBuf::from(common_dir.trim())
        .parent()?
        .canonicalize()
        .ok()?;
    if root == canonical_path {
        None
    } else {
        Some(root)
    }
}

/// `worktree_main_repo`, run once per path; per-query settings lookups call
/// this and would otherwise spawn `git` each time.
pub fn worktree_main_repo_cached(path: &Path) -> Option<PathBuf> {
    static ANSWERS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<PathBuf, Option<PathBuf>>>,
    > = std::sync::OnceLock::new();
    let answers = ANSWERS.get_or_init(Default::default);
    if let Some(answer) = answers.lock().unwrap().get(path) {
        return answer.clone();
    }
    let answer = worktree_main_repo(path);
    answers
        .lock()
        .unwrap()
        .insert(path.to_path_buf(), answer.clone());
    answer
}

/// `(worktree root, branch)` of each entry of `git worktree list --porcelain`.
/// A detached or bare entry has no branch.
fn parse_worktree_porcelain(output: &str) -> Vec<(PathBuf, Option<String>)> {
    let mut worktrees: Vec<(PathBuf, Option<String>)> = Vec::new();
    for line in output.lines() {
        if let Some(root) = line.strip_prefix("worktree ") {
            worktrees.push((PathBuf::from(root), None));
        } else if let Some(branch) = line.strip_prefix("branch ") {
            if let Some(last) = worktrees.last_mut() {
                let short = branch.strip_prefix("refs/heads/").unwrap_or(branch);
                last.1 = Some(short.to_string());
            }
        }
    }
    worktrees
}

/// Where `repo` is in the worktree that has `branch` checked out: that
/// worktree's root plus the subdirectory `repo` has in its own worktree.
/// `None` when no worktree of `repo` has `branch` checked out.
pub fn worktree_for_branch(repo: &Path, branch: &str) -> Result<Option<PathBuf>> {
    let output = std::process::Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo)
        .output()?;
    if !output.status.success() {
        return Err(anyhow!(
            "git worktree list failed in {:?}: {}",
            repo,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let worktrees = parse_worktree_porcelain(&String::from_utf8_lossy(&output.stdout));
    let repo = repo.canonicalize()?;

    // The deepest worktree holding `repo`: a worktree can sit inside another.
    let subdir = worktrees
        .iter()
        .filter_map(|(root, _)| {
            let root = root.canonicalize().ok()?;
            repo.strip_prefix(&root).ok().map(Path::to_path_buf)
        })
        .min_by_key(|subdir| subdir.components().count());
    let Some(subdir) = subdir else {
        return Ok(None);
    };
    let Some((root, _)) = worktrees
        .into_iter()
        .find(|(_, checked_out)| checked_out.as_deref() == Some(branch))
    else {
        return Ok(None);
    };
    let root = root.canonicalize().unwrap_or(root);
    // Joining an empty subdir would append a trailing '/' to the repo key.
    if subdir.as_os_str().is_empty() {
        return Ok(Some(root));
    }
    Ok(Some(root.join(subdir)))
}

/// Validate a repository path
pub fn validate_repo_path(path: &Path) -> Result<()> {
    if !path.exists() {
        return Err(anyhow!("Path does not exist: {:?}", path));
    }

    if !path.is_dir() {
        return Err(anyhow!("Path is not a directory: {:?}", path));
    }

    // Check if readable
    std::fs::read_dir(path).map_err(|e| anyhow!("Cannot read directory {:?}: {}", path, e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_is_repository_git() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        assert!(is_repository(dir.path()));
    }

    #[test]
    fn test_is_repository_cargo() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        assert!(is_repository(dir.path()));
    }

    #[test]
    fn test_is_not_repository() {
        let dir = tempdir().unwrap();
        assert!(!is_repository(dir.path()));
    }

    fn run_git(dir: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@e")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@e")
            .status()
            .unwrap();
        assert!(status.success(), "git {:?} failed in {:?}", args, dir);
    }

    #[test]
    fn worktree_main_repo_is_none_for_a_plain_checkout() {
        let dir = tempdir().unwrap();
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["commit", "--allow-empty", "-q", "-m", "init"]);
        assert!(worktree_main_repo(dir.path()).is_none());
    }

    #[test]
    fn worktree_main_repo_finds_the_main_checkout_of_a_linked_worktree() {
        let main = tempdir().unwrap();
        run_git(main.path(), &["init", "-q"]);
        run_git(
            main.path(),
            &["commit", "--allow-empty", "-q", "-m", "init"],
        );

        let worktree_parent = tempdir().unwrap();
        let worktree_path = worktree_parent.path().join("wt");
        run_git(
            main.path(),
            &[
                "worktree",
                "add",
                "-q",
                worktree_path.to_str().unwrap(),
                "-b",
                "wt-branch",
            ],
        );

        let resolved = worktree_main_repo(&worktree_path).unwrap();
        assert_eq!(resolved, main.path().canonicalize().unwrap());
    }

    #[test]
    fn parse_worktree_porcelain_reads_root_and_branch() {
        let output = "worktree /src/widget\nHEAD 0123\nbranch refs/heads/main\n\n\
                      worktree /src/widget-wt\nHEAD 4567\ndetached\n\n\
                      worktree /src/widget-fix\nHEAD 89ab\nbranch refs/heads/fix/gadget\n";
        assert_eq!(
            parse_worktree_porcelain(output),
            vec![
                (PathBuf::from("/src/widget"), Some("main".to_string())),
                (PathBuf::from("/src/widget-wt"), None),
                (
                    PathBuf::from("/src/widget-fix"),
                    Some("fix/gadget".to_string())
                ),
            ]
        );
    }

    #[test]
    fn worktree_for_branch_keeps_the_subdirectory_of_a_monorepo_package() {
        let main = tempdir().unwrap();
        run_git(main.path(), &["init", "-q"]);
        std::fs::create_dir_all(main.path().join("pkg/widget")).unwrap();
        std::fs::write(main.path().join("pkg/widget/lib.rs"), "").unwrap();
        run_git(main.path(), &["add", "."]);
        run_git(main.path(), &["commit", "-q", "-m", "init"]);
        let worktree_parent = tempdir().unwrap();
        let worktree_path = worktree_parent.path().join("wt");
        run_git(
            main.path(),
            &[
                "worktree",
                "add",
                "-q",
                worktree_path.to_str().unwrap(),
                "-b",
                "wt-branch",
            ],
        );

        let package = main.path().join("pkg/widget");
        assert_eq!(
            worktree_for_branch(&package, "wt-branch").unwrap(),
            Some(worktree_path.canonicalize().unwrap().join("pkg/widget"))
        );
        assert_eq!(
            worktree_for_branch(&package, "no-such-branch").unwrap(),
            None
        );
    }
}
