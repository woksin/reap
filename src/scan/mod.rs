pub mod artifacts;
pub mod caches;
pub mod docker;
pub mod git;

use crate::model::{Category, ScanEvent};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread;

#[derive(Clone)]
pub struct ScanOpts {
    /// Measured directory sizes, reused across runs where still valid.
    pub cache: std::sync::Arc<crate::cache::SizeCache>,
    /// Directories to search for repositories and build artifacts.
    pub roots: Vec<PathBuf>,
    /// A branch, worktree or artifact must be untouched this long to count as stale.
    pub stale_days: u64,
    /// Ignore anything smaller than this, to keep the list signal-dense.
    pub min_size: u64,
    /// How deep to descend from each root.
    pub max_depth: usize,
    pub skip_docker: bool,
    pub skip_caches: bool,
}

impl Default for ScanOpts {
    fn default() -> Self {
        Self {
            cache: std::sync::Arc::new(crate::cache::SizeCache::load(false)),
            roots: default_roots(),
            stale_days: 30,
            min_size: 1024 * 1024, // 1 MB
            max_depth: 8,
            skip_docker: false,
            skip_caches: false,
        }
    }
}

/// Common places developers keep checkouts. Only the ones that exist are kept.
pub fn default_roots() -> Vec<PathBuf> {
    let Some(home) = home_dir() else {
        return vec![];
    };
    [
        "repos",
        "src",
        "Developer",
        "Projects",
        "projects",
        "code",
        "Code",
        "dev",
        "work",
        "git",
    ]
    .iter()
    .map(|d| home.join(d))
    .filter(|p| p.is_dir())
    .collect()
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).filter(|p| p.is_dir())
}

/// Kick off every scanner on its own thread so results stream into the UI.
pub fn spawn_all(opts: ScanOpts, tx: Sender<ScanEvent>) {
    // Git and artifacts share a repository walk, so they run on one thread and
    // report independently.
    {
        let opts = opts.clone();
        let tx = tx.clone();
        thread::spawn(move || {
            let repos = discover_repos(&opts);
            let _ = tx.send(ScanEvent::Status(format!("{} repositories", repos.len())));
            git::scan(&repos, &opts, &tx);
            let _ = tx.send(ScanEvent::Done(Category::Git));
        });
    }
    {
        let opts = opts.clone();
        let tx = tx.clone();
        thread::spawn(move || {
            artifacts::scan(&opts, &tx);
            let _ = tx.send(ScanEvent::Done(Category::Artifacts));
        });
    }
    {
        let opts = opts.clone();
        let tx = tx.clone();
        thread::spawn(move || {
            if !opts.skip_docker {
                docker::scan(&opts, &tx);
            }
            let _ = tx.send(ScanEvent::Done(Category::Docker));
        });
    }
    {
        thread::spawn(move || {
            if !opts.skip_caches {
                caches::scan(&opts, &tx);
            }
            let _ = tx.send(ScanEvent::Done(Category::Caches));
        });
    }
}

/// Shallow hunt for `.git` directories under the configured roots.
///
/// Linked worktrees each look like a repository but share one object store with
/// their main worktree. Left alone that reports every branch, stash and `gc`
/// opportunity once per checkout, so each set is collapsed to a single entry.
pub fn discover_repos(opts: &ScanOpts) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for root in &opts.roots {
        walk_for_git(root, 0, opts.max_depth.min(5), &mut found);
    }
    found.sort();
    found.dedup();

    let mut by_store: std::collections::BTreeMap<PathBuf, PathBuf> = std::collections::BTreeMap::new();
    for repo in found {
        let store = common_git_dir(&repo).unwrap_or_else(|| repo.join(".git"));
        // Prefer the main worktree as the canonical entry: it is the one whose
        // directory contains the shared object store.
        let canonical = store
            .file_name()
            .filter(|n| *n == ".git")
            .and_then(|_| store.parent())
            .filter(|p| p.is_dir())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| repo.clone());
        by_store.entry(store).or_insert(canonical);
    }
    by_store.into_values().collect()
}

/// The object store shared by a repository and all its linked worktrees.
fn common_git_dir(repo: &Path) -> Option<PathBuf> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--git-common-dir"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if raw.is_empty() {
        return None;
    }
    // Older git returns a path relative to the repository.
    let path = PathBuf::from(&raw);
    let path = if path.is_absolute() {
        path
    } else {
        repo.join(path)
    };
    std::fs::canonicalize(&path).ok().or(Some(path))
}

fn walk_for_git(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<PathBuf>) {
    if depth > max_depth {
        return;
    }
    if dir.join(".git").exists() {
        out.push(dir.to_path_buf());
        // Nested repos are rare and submodules are managed by the parent, so a
        // repository terminates the walk.
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        // `is_dir` is false for symlinks, which keeps cycles out of the walk.
        if !ft.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || is_never_descend(&name) {
            continue;
        }
        walk_for_git(&entry.path(), depth + 1, max_depth, out);
    }
}

/// Directories that never contain a checkout worth scanning and are expensive
/// to traverse.
pub fn is_never_descend(name: &str) -> bool {
    matches!(
        name,
        "node_modules"
            | "target"
            | "Library"
            | "Applications"
            | "System"
            | "Volumes"
            | ".Trash"
            | "vendor"
            | "Pods"
    )
}
