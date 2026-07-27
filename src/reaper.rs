use crate::model::{Action, Candidate, ReapEvent};
use crate::scan::home_dir;
use crate::trash;
use rayon::prelude::*;
use std::cmp::Reverse;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::Sender;
use std::thread;

/// Paths that must never be handed to a recursive delete, however we got here.
fn is_forbidden(path: &Path) -> bool {
    if !path.is_absolute() {
        return true;
    }
    // `/`, `/Users`, `/Users/name` — anything this shallow is a mistake.
    let depth = path.components().count();
    if depth < 3 {
        return true;
    }
    if let Some(home) = home_dir()
        && path == home
    {
        return true;
    }
    matches!(
        path.to_str().unwrap_or(""),
        "/" | "/usr"
            | "/etc"
            | "/var"
            | "/bin"
            | "/sbin"
            | "/opt"
            | "/boot"
            | "/proc"
            | "/sys"
            | "/home"
            | "/root"
            | "/srv"
            | "/System"
            | "/Library"
            | "/Applications"
    )
}

#[derive(Clone, Copy)]
pub struct ReapOpts {
    pub dry_run: bool,
    /// Move paths to the volume's trash instead of unlinking them. Recoverable,
    /// but the space only comes back once the trash is emptied.
    pub trash: bool,
}

enum Outcome {
    Done,
    /// Moved to the trash; the bytes are still on disk until it is emptied.
    Trashed(PathBuf),
    /// Nothing to do — an ancestor already took it. Costs no space.
    Skipped,
    Failed(String),
}

/// What happened to one candidate.
pub struct Report {
    pub label: String,
    /// Bytes this candidate is expected to account for.
    pub freed: u64,
    pub ok: bool,
    pub error: Option<String>,
    /// Where it landed, when trashing rather than deleting.
    pub trashed: Option<PathBuf>,
}

/// Position of a `git stash drop stash@{N}` in the stash list, if that is what
/// this action is.
fn stash_index(args: &[String]) -> Option<u64> {
    if args.first().map(String::as_str) != Some("stash")
        || args.get(1).map(String::as_str) != Some("drop")
    {
        return None;
    }
    let reflog = args.get(2)?;
    reflog
        .strip_prefix("stash@{")?
        .strip_suffix('}')?
        .parse()
        .ok()
}

/// Order for the serial command phase.
///
/// Dropping a stash renumbers every stash below it, so `stash@{0}` followed by
/// `stash@{1}` would delete something the user never selected. Running the
/// highest index first leaves the lower ones where they were.
fn command_order(action: &Action) -> Reverse<u64> {
    match action {
        Action::Run { args, .. } => Reverse(stash_index(args).unwrap_or(0)),
        Action::Remove(_) => Reverse(0),
    }
}

/// Split removals into the ones to perform and the ones an ancestor covers.
///
/// Selections legitimately overlap — a stale worktree contains its own
/// `node_modules` — and the nested entry must neither be deleted twice nor
/// count its bytes twice. What remains is pairwise disjoint, so it is safe to
/// run in parallel.
fn partition_removals(mut removals: Vec<Candidate>) -> (Vec<Candidate>, Vec<Candidate>) {
    removals.sort_by_key(|c| match &c.action {
        Action::Remove(p) => p.components().count(),
        Action::Run { .. } => 0,
    });

    let mut roots: Vec<PathBuf> = Vec::new();
    let mut covered = Vec::new();
    let mut independent = Vec::new();

    for cand in removals {
        let Action::Remove(path) = &cand.action else {
            continue;
        };
        if roots.iter().any(|r| path.starts_with(r)) {
            covered.push(cand);
        } else {
            roots.push(path.clone());
            independent.push(cand);
        }
    }
    (independent, covered)
}

/// Run every candidate, reporting each through `emit` as it finishes.
pub fn run_all(items: Vec<Candidate>, opts: ReapOpts, emit: impl Fn(Report) + Sync + Send) {
    let (removals, mut commands): (Vec<_>, Vec<_>) = items
        .into_iter()
        .partition(|c| matches!(c.action, Action::Remove(_)));

    // Commands touch shared state — a repository's ref store, the Docker
    // daemon — and their order matters, so they stay serial.
    commands.sort_by_key(|c| command_order(&c.action));
    for cand in commands {
        emit(report_for(&cand, execute(&cand.action, opts)));
    }

    let (independent, covered) = partition_removals(removals);

    // Disjoint directory trees, so unlinking them concurrently is safe and
    // considerably faster when there are hundreds of thousands of inodes.
    independent.par_iter().for_each(|cand| {
        emit(report_for(cand, execute(&cand.action, opts)));
    });

    for cand in covered {
        emit(report_for(&cand, Outcome::Skipped));
    }
}

fn report_for(cand: &Candidate, outcome: Outcome) -> Report {
    let (freed, ok, error, trashed) = match outcome {
        Outcome::Done => (cand.size, true, None, None),
        Outcome::Trashed(dest) => (cand.size, true, None, Some(dest)),
        Outcome::Skipped => (0, true, None, None),
        Outcome::Failed(e) => (0, false, Some(e), None),
    };
    Report {
        label: cand.label.clone(),
        freed,
        ok,
        error,
        trashed,
    }
}

/// Run the selected candidates on a background thread, reporting each result.
pub fn spawn(items: Vec<Candidate>, opts: ReapOpts, tx: Sender<ReapEvent>) {
    thread::spawn(move || {
        // `Sender` is Send but not Sync, so the parallel phase serialises its
        // sends through a lock rather than cloning per task.
        let guard = std::sync::Mutex::new(tx);
        run_all(items, opts, |report| {
            if let Ok(tx) = guard.lock() {
                let _ = tx.send(ReapEvent::Progress(Box::new(report)));
            }
        });
        if let Ok(tx) = guard.lock() {
            let _ = tx.send(ReapEvent::Finished);
        }
    });
}

fn execute(action: &Action, opts: ReapOpts) -> Outcome {
    match action {
        Action::Remove(path) => {
            if is_forbidden(path) {
                return Outcome::Failed("refused: path is too broad to delete".into());
            }
            if !path.exists() {
                return Outcome::Skipped;
            }
            if opts.dry_run {
                return Outcome::Done;
            }
            if opts.trash {
                return match trash::move_to_trash(path) {
                    Ok(dest) => Outcome::Trashed(dest),
                    // Never quietly fall back to an unrecoverable delete when
                    // the user asked for a recoverable one.
                    Err(e) => Outcome::Failed(format!("could not trash: {e}")),
                };
            }
            match std::fs::remove_dir_all(path) {
                Ok(()) => Outcome::Done,
                // Not a directory — try it as a single file before giving up.
                Err(_) => match std::fs::remove_file(path) {
                    Ok(()) => Outcome::Done,
                    Err(e) => Outcome::Failed(e.to_string()),
                },
            }
        }
        Action::Run { program, args, cwd } => {
            if opts.dry_run {
                return Outcome::Done;
            }
            let mut cmd = Command::new(program);
            cmd.args(args);
            if let Some(dir) = cwd {
                cmd.current_dir(dir);
            }
            match cmd.output() {
                Ok(out) if out.status.success() => Outcome::Done,
                Ok(out) => {
                    let err = String::from_utf8_lossy(&out.stderr);
                    let msg = err
                        .lines()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or("command failed")
                        .to_string();
                    Outcome::Failed(msg)
                }
                Err(e) => Outcome::Failed(e.to_string()),
            }
        }
    }
}

/// Permanently remove the entries this run put in the trash.
///
/// Scoped to exactly those paths so nothing the user trashed themselves is
/// touched. Returns how many were removed.
pub fn empty_trashed(paths: &[PathBuf]) -> usize {
    paths
        .par_iter()
        .filter(|p| {
            // Guard against ever being handed something outside a trash.
            // Covers macOS (`.Trash`, `.Trashes`) and freedesktop
            // (`Trash`, `.Trash-1000`).
            p.components().any(|c| {
                let s = c.as_os_str().to_string_lossy();
                s == "Trash" || s == ".Trash" || s == ".Trashes" || s.starts_with(".Trash-")
            })
        })
        .filter(|p| {
            std::fs::remove_dir_all(p)
                .or_else(|_| std::fs::remove_file(p))
                .is_ok()
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Category, Risk};
    use std::sync::Mutex;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("reap-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn candidate(label: &str, size: u64, path: &Path) -> Candidate {
        Candidate::new(
            Category::Artifacts,
            "test",
            label,
            "",
            size,
            Risk::Safe,
            Action::Remove(path.to_path_buf()),
        )
    }

    fn plain(dry_run: bool) -> ReapOpts {
        ReapOpts {
            dry_run,
            trash: false,
        }
    }

    fn run(items: Vec<Candidate>, opts: ReapOpts) -> Vec<Report> {
        let out = Mutex::new(Vec::new());
        run_all(items, opts, |r| out.lock().unwrap().push(r));
        out.into_inner().unwrap()
    }

    #[test]
    fn refuses_paths_that_are_too_broad() {
        for p in ["/", "/usr", "/etc", "/Users", "/Volumes", "/System"] {
            assert!(is_forbidden(Path::new(p)), "{p} should be refused");
        }
        assert!(is_forbidden(Path::new("relative/path")));
        if let Some(home) = home_dir() {
            assert!(is_forbidden(&home), "$HOME itself should be refused");
        }
    }

    #[test]
    fn allows_a_normal_nested_target() {
        assert!(!is_forbidden(Path::new(
            "/Users/someone/code/app/node_modules"
        )));
    }

    #[test]
    fn forbidden_paths_fail_rather_than_delete() {
        let out = run(vec![candidate("root", 100, Path::new("/"))], plain(false));
        assert_eq!(out.len(), 1);
        assert!(!out[0].ok, "deleting / must not report success");
        assert_eq!(out[0].freed, 0);
        assert!(out[0].error.as_ref().unwrap().contains("too broad"));
        assert!(Path::new("/usr").exists(), "filesystem must be untouched");
    }

    #[test]
    fn nested_selection_counts_its_bytes_once() {
        let root = tmp("nested");
        let parent = root.join("project/node_modules");
        let child = parent.join("nested/deps");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(child.join("f.txt"), b"data").unwrap();

        // Deliberately queued child-first to prove ordering is applied.
        let out = run(
            vec![
                candidate("child", 400, &child),
                candidate("parent", 1000, &parent),
            ],
            plain(false),
        );

        let total: u64 = out.iter().map(|r| r.freed).sum();
        assert_eq!(total, 1000, "child bytes must not be counted again");
        assert!(out.iter().all(|r| r.ok));
        assert!(!parent.exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn independent_directories_are_all_removed() {
        let root = tmp("parallel");
        let mut items = Vec::new();
        for i in 0..24 {
            let d = root.join(format!("project{i}/node_modules"));
            std::fs::create_dir_all(d.join("pkg")).unwrap();
            std::fs::write(d.join("pkg/index.js"), b"x").unwrap();
            items.push(candidate(&format!("p{i}"), 10, &d));
        }

        let out = run(items, plain(false));
        assert_eq!(out.len(), 24);
        assert!(out.iter().all(|r| r.ok), "every removal should succeed");
        assert_eq!(out.iter().map(|r| r.freed).sum::<u64>(), 240);
        for i in 0..24 {
            assert!(!root.join(format!("project{i}/node_modules")).exists());
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn dry_run_reports_savings_without_deleting() {
        let root = tmp("dryrun");
        let target = root.join("project/target");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("f.bin"), b"payload").unwrap();

        let out = run(vec![candidate("target", 512, &target)], plain(true));
        assert_eq!(out[0].freed, 512);
        assert!(out[0].ok);
        assert!(target.exists(), "dry run must leave the directory in place");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn trashing_keeps_the_contents_recoverable() {
        let root = tmp("trash");
        // Unique name: the trash is shared with the other tests.
        let target = root.join(format!("project/reap-nm-{}", std::process::id()));
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("keep.txt"), b"recoverable").unwrap();

        let out = run(
            vec![candidate("nm", 4096, &target)],
            ReapOpts {
                dry_run: false,
                trash: true,
            },
        );
        assert!(out[0].ok);
        let dest = out[0].trashed.clone().expect("should record a trash path");
        assert!(!target.exists(), "original must be gone");
        assert_eq!(
            std::fs::read_to_string(dest.join("keep.txt")).unwrap(),
            "recoverable"
        );

        // And emptying only what we trashed clears it for good.
        assert_eq!(empty_trashed(std::slice::from_ref(&dest)), 1);
        assert!(!dest.exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn emptying_refuses_paths_outside_a_trash() {
        let root = tmp("notrash");
        let decoy = root.join("important");
        std::fs::create_dir_all(&decoy).unwrap();

        assert_eq!(empty_trashed(std::slice::from_ref(&decoy)), 0);
        assert!(decoy.exists(), "must not delete outside a trash directory");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn already_missing_paths_succeed_but_free_nothing() {
        let root = tmp("missing");
        let out = run(
            vec![candidate("ghost", 999, &root.join("not-there"))],
            plain(false),
        );
        assert!(out[0].ok, "a path already gone is not a failure");
        assert_eq!(
            out[0].freed, 0,
            "it frees nothing, so it must not be counted"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn failing_command_is_reported_not_swallowed() {
        let item = Candidate::new(
            Category::Git,
            "test",
            "bad command",
            "",
            100,
            Risk::Safe,
            Action::Run {
                program: "false".into(),
                args: vec![],
                cwd: None,
            },
        );
        let out = run(vec![item], plain(false));
        assert!(!out[0].ok);
        assert_eq!(out[0].freed, 0);
    }

    fn stash(reflog: &str) -> Candidate {
        Candidate::new(
            Category::Git,
            "old stashes",
            reflog,
            "",
            0,
            Risk::Danger,
            Action::Run {
                program: "git".into(),
                args: vec!["stash".into(), "drop".into(), reflog.into()],
                cwd: None,
            },
        )
    }

    #[test]
    fn stashes_are_dropped_highest_index_first() {
        // Dropping stash@{0} first would renumber the rest and delete the
        // wrong entries. Dry run keeps this from touching a real repository.
        let items = vec![stash("stash@{0}"), stash("stash@{2}"), stash("stash@{1}")];
        let out = run(items, plain(true));
        let order: Vec<&str> = out.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(order, ["stash@{2}", "stash@{1}", "stash@{0}"]);
    }

    #[test]
    fn stash_index_only_matches_real_stash_drops() {
        assert_eq!(
            stash_index(&["stash".into(), "drop".into(), "stash@{7}".into()]),
            Some(7)
        );
        assert_eq!(
            stash_index(&["branch".into(), "-d".into(), "x".into()]),
            None
        );
        assert_eq!(stash_index(&["stash".into(), "list".into()]), None);
        assert_eq!(
            stash_index(&["stash".into(), "drop".into(), "refs/stash".into()]),
            None
        );
    }

    #[test]
    fn commands_run_before_path_removals() {
        // `git worktree remove` must get its chance before a bare recursive
        // delete of something inside that worktree.
        let root = tmp("phases");
        let nested = root.join("wt/node_modules");
        std::fs::create_dir_all(&nested).unwrap();

        let wt = Candidate::new(
            Category::Git,
            "stale worktrees",
            "worktree",
            "",
            0,
            Risk::Caution,
            Action::Run {
                program: "true".into(),
                args: vec![],
                cwd: None,
            },
        );
        let out = run(vec![candidate("nested", 0, &nested), wt], plain(true));
        assert_eq!(out[0].label, "worktree");
        std::fs::remove_dir_all(&root).ok();
    }
}
