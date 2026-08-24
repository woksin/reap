//! Contexts the specifications are established in.
//!
//! Fixtures build real git repositories and real directory trees rather than
//! mocking them. The behaviour under specification is largely *what git and the
//! filesystem say*, so a mock would only assert that the fixture agrees with
//! itself. Where the real thing cannot be built inside a test — a docker
//! daemon — the fixture is output captured from one.

pub mod a_docker_daemon;
pub mod a_download_directory;
pub mod a_project;
pub mod a_repository;
pub mod an_agent_home;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

/// A scratch directory that removes itself when the context ends.
pub struct scratch {
    pub path: PathBuf,
}

impl scratch {
    pub fn named(what: &str) -> Self {
        // A counter rather than the clock. `as_nanos` reports nanoseconds but
        // does not advance in them, so two specs starting inside the same tick
        // used to be handed the same directory — and each would then see the
        // other's fixture as part of its own, which fails whichever of them
        // counts what it found.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let path =
            std::env::temp_dir().join(format!("reap-spec-{what}-{}-{unique}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a scratch directory");
        Self { path }
    }
}

impl Drop for scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Run git in `dir`, failing loudly.
///
/// A fixture that fails quietly produces specifications that fail for reasons
/// having nothing to do with what they specify.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        // Independent of whatever the machine's global git config says, so a
        // signing key or a different default branch cannot change the outcome.
        .args(["-c", "commit.gpgsign=false"])
        .args(["-c", "user.name=Spec"])
        .args(["-c", "user.email=spec@example.com"])
        .args(["-c", "init.defaultBranch=main"])
        .args(["-c", "advice.detachedHead=false"])
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("could not run git {args:?}: {e}"));

    assert!(
        out.status.success(),
        "git {args:?} failed in {}\nstdout: {}\nstderr: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Scan options that hold nothing back, so a specification sees every candidate
/// the scanners produce rather than whatever survives the default thresholds.
pub fn scanning_everything(roots: Vec<PathBuf>) -> crate::scan::ScanOpts {
    crate::scan::ScanOpts {
        rules: std::sync::Arc::new(crate::scan::Rules::default()),
        cache: std::sync::Arc::new(crate::cache::SizeCache::load(false)),
        roots,
        // Nothing is excluded for being recent or small: the specifications
        // control what exists, and want to observe all of it.
        stale_days: 0,
        min_size: 0,
        max_depth: 8,
        skip_docker: true,
        skip_caches: true,
        skip_agents: true,
        skip_personal: true,
        // Only the fixture's own tree, never the machine running the specs.
        scan_home_strays: false,
    }
}

/// Move a path's modification time `seconds_ago` into the past.
///
/// The scanners' staleness rules read mtime, so a fixture that wrote everything
/// "now" could only ever specify the recent case. Seconds rather than days
/// because one threshold in play is a threshold in hours: a session that ended
/// this morning is not a session that is finished.
///
/// Zero is a real instruction to stamp the path with the current time, not a
/// request to leave it alone — a fixture asks for that *after* aging the tree
/// around it, and "leave it alone" would silently hand back the old timestamp.
fn back_date(path: &Path, seconds_ago: u64) {
    let when = SystemTime::now() - Duration::from_secs(seconds_ago);
    let file = open_to_set_times(path).expect("the fixture's own path");
    file.set_modified(when).expect("back-dating the fixture");
}

/// Back-date everything under `dir`, and `dir` itself, deepest first.
///
/// The order is the point. Writing a file updates the directory holding it, so
/// aging a parent before its children undoes itself on the way back down.
fn back_date_tree(dir: &Path, seconds_ago: u64) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(ft) if ft.is_dir() => back_date_tree(&path, seconds_ago),
                Ok(ft) if ft.is_file() => back_date(&path, seconds_ago),
                _ => {}
            }
        }
    }
    back_date(dir, seconds_ago);
}

/// Open a file *or a directory* in a way that permits setting its timestamps.
///
/// The two platforms disagree about what that takes, and both refuse the
/// other's answer. `futimens` works through a read-only descriptor, and a
/// directory cannot be opened any other way on unix. Windows wants the
/// attribute-write right named explicitly, and will not open a directory at all
/// without backup semantics.
#[cfg(unix)]
fn open_to_set_times(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

#[cfg(windows)]
fn open_to_set_times(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

    std::fs::OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
}
