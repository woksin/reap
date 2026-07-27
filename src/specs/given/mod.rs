//! Contexts the specifications are established in.
//!
//! Fixtures build real git repositories and real directory trees rather than
//! mocking them. The behaviour under specification is largely *what git and the
//! filesystem say*, so a mock would only assert that the fixture agrees with
//! itself. Where the real thing cannot be built inside a test — a docker
//! daemon — the fixture is output captured from one.

pub mod a_docker_daemon;
pub mod a_project;
pub mod a_repository;

use std::path::{Path, PathBuf};
use std::process::Command;

/// A scratch directory that removes itself when the context ends.
pub struct scratch {
    pub path: PathBuf,
}

impl scratch {
    pub fn named(what: &str) -> Self {
        // Nanoseconds keep parallel specs from colliding in the shared temp dir.
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
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
        // Only the fixture's own tree, never the machine running the specs.
        scan_home_strays: false,
    }
}
