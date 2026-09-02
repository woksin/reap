use crate::model::{Action, BranchProof, Candidate, ReapEvent};
use crate::scan::home_dir;
use crate::trash;
use rayon::prelude::*;
use std::cmp::Reverse;
use std::fmt::Write as FmtWrite;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::thread;

/// The shallowest path that could ever be a legitimate target.
///
/// Unix counts the root as one component, so `/Users/name` is three. Windows
/// counts the drive and the root separately, so `C:\Users\name` is four — the
/// same depth, one more component to say it in.
#[cfg(windows)]
const MIN_COMPONENTS: usize = 4;
#[cfg(not(windows))]
const MIN_COMPONENTS: usize = 3;

/// Top-level Windows directories that belong to the system, not to anyone.
///
/// `Users` is deliberately absent: everything reap offers on Windows lives under
/// a profile, and `C:\Users` itself is already too shallow to reach here.
#[cfg(windows)]
const WINDOWS_SYSTEM_ROOTS: &[&str] = &[
    "windows",
    "program files",
    "program files (x86)",
    "programdata",
    "$recycle.bin",
    "system volume information",
    "recovery",
    "boot",
];

/// Is this path inside a top-level directory that belongs to the system?
#[cfg(windows)]
fn in_system_root(path: &Path) -> bool {
    path.components()
        .find_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().to_ascii_lowercase()),
            _ => None,
        })
        .is_some_and(|first| WINDOWS_SYSTEM_ROOTS.contains(&first.as_str()))
}

/// Unix directories that belong to the system along with everything inside
/// them.
///
/// Nothing reap offers lives below any of these: every cache and artifact rule
/// is written against `$HOME`. So the whole subtree is refused rather than only
/// the root, which is what the Windows arm above has always done. Matching only
/// the root left real paths reachable — `/usr/lib/node_modules` is where a
/// global npm install puts itself, and a scan root added by hand can walk
/// straight into it.
#[cfg(not(windows))]
const SYSTEM_SUBTREES: &[&str] = &[
    "/bin",
    "/sbin",
    "/boot",
    "/proc",
    "/sys",
    "/etc",
    "/usr",
    "/System",
    "/Library",
    "/Applications",
];

/// Unix directories that must survive themselves, while their children are
/// ordinary.
///
/// These cannot be subtrees. `$HOME` is `/home/name` on Linux and `/root` for
/// root, so refusing those subtrees would refuse everything reap exists to
/// find. `/opt` holds Homebrew. `/var` is where macOS puts `$TMPDIR`, as
/// `/var/folders/…`, which is an ordinary place to own a directory.
#[cfg(not(windows))]
const SYSTEM_ROOTS: &[&str] = &["/", "/home", "/root", "/srv", "/opt", "/var"];

#[cfg(not(windows))]
fn in_system_root(path: &Path) -> bool {
    // `Path::starts_with` compares whole components, so `/usrlocal` is not
    // caught by `/usr`, and a path that is not valid UTF-8 is still compared
    // rather than silently allowed the way a `to_str` match would allow it.
    SYSTEM_ROOTS.iter().any(|root| path == Path::new(root))
        || SYSTEM_SUBTREES.iter().any(|root| path.starts_with(root))
}

/// Paths that must never be handed to a recursive delete, however we got here.
fn is_forbidden(path: &Path) -> bool {
    if !path.is_absolute() {
        return true;
    }
    // `/`, `/Users`, `/Users/name` — anything this shallow is a mistake.
    if path.components().count() < MIN_COMPONENTS {
        return true;
    }
    if let Some(home) = home_dir()
        && path == home
    {
        return true;
    }
    in_system_root(path)
}

/// Would this path be forbidden once the kernel has resolved the way there?
///
/// [`is_forbidden`] matches on the literal path, which is the whole story for
/// the leaf: a symlink is unlinked rather than followed, so its target is never
/// at risk. It is not the story for the *ancestors*, which the kernel resolves
/// during traversal. A redirected `~/.cache` — a link some tool left behind, or
/// a deliberate move onto another volume — lets a cache sweep delete whatever
/// it points at while the string being checked still reads as an ordinary path
/// under `$HOME`.
///
/// Deny-only, deliberately. A resolved path may add a refusal and can never
/// remove one, so an ordinary target whose parent merely happens to be a link
/// keeps the verdict its literal spelling already earned. That is what makes
/// this safe to run before the allowances rather than after them.
fn resolves_into_forbidden(path: &Path) -> bool {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return false;
    };
    // A parent that will not resolve is not evidence of anything — most often
    // it simply does not exist, which `execute` already treats as nothing to
    // do. Refusing here would turn an absent path into a reported failure.
    let Ok(resolved) = std::fs::canonicalize(parent) else {
        return false;
    };
    resolved != parent && is_forbidden(&resolved.join(name))
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
    /// `None` where the platform will not say where it put it — recoverable by
    /// the user, but not something reap can offer to empty afterwards.
    Trashed(Option<PathBuf>),
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
        Action::None
        | Action::Remove(_)
        | Action::GitBranchDelete { .. }
        | Action::GitWorktreeRemove { .. } => Reverse(0),
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
        Action::None
        | Action::GitBranchDelete { .. }
        | Action::GitWorktreeRemove { .. }
        | Action::Run { .. } => 0,
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
    // Command actions can remove filesystem trees too (`git worktree remove`).
    // Resolve those footprints before splitting commands from path removals, or
    // a selected worktree and its selected node_modules are counted and acted
    // on twice.
    let mut ordered = items;
    ordered.sort_by_key(|candidate| {
        candidate
            .footprint
            .as_deref()
            .map_or(usize::MAX, |path| path.components().count())
    });
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut actionable = Vec::new();
    let mut covered = Vec::new();
    for candidate in ordered {
        if matches!(candidate.action, Action::None) {
            covered.push(candidate);
            continue;
        }
        if candidate
            .footprint
            .as_deref()
            .is_some_and(|path| roots.iter().any(|root| path.starts_with(root)))
        {
            covered.push(candidate);
        } else {
            if let Some(path) = &candidate.footprint {
                roots.push(path.clone());
            }
            actionable.push(candidate);
        }
    }

    let (removals, mut commands): (Vec<_>, Vec<_>) = actionable
        .into_iter()
        .partition(|c| matches!(c.action, Action::Remove(_)));

    // Commands touch shared state — a repository's ref store, the Docker
    // daemon — and their order matters, so they stay serial.
    commands.sort_by_key(|c| command_order(&c.action));
    for cand in commands {
        emit(report_for(&cand, execute(&cand.action, opts)));
    }

    let (independent, nested_removals) = partition_removals(removals);

    // Disjoint directory trees, so unlinking them concurrently is safe and
    // considerably faster when there are hundreds of thousands of inodes.
    independent.par_iter().for_each(|cand| {
        emit(report_for(cand, execute(&cand.action, opts)));
    });

    for cand in covered.into_iter().chain(nested_removals) {
        emit(report_for(&cand, Outcome::Skipped));
    }
}

fn report_for(cand: &Candidate, outcome: Outcome) -> Report {
    let (freed, ok, error, trashed) = match outcome {
        Outcome::Done => (cand.size, true, None, None),
        Outcome::Trashed(dest) => (cand.size, true, None, dest),
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

fn git_output(repo: &Path, args: &[&str]) -> Option<std::process::Output> {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .ok()
}

fn branch_proof_holds(repo: &Path, oid: &str, proof: &BranchProof) -> bool {
    match proof {
        BranchProof::ContainsRef { reference, .. } => {
            git_output(repo, &["merge-base", "--is-ancestor", oid, reference])
                .is_some_and(|output| output.status.success())
        }
        BranchProof::PatchesIn { reference, .. } => {
            let range = format!("{reference}..{oid}");
            let linear = git_output(repo, &["rev-list", "--count", "--merges", &range])
                .filter(|output| output.status.success())
                .is_some_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "0");
            let patches = git_output(repo, &["cherry", reference, oid])
                .filter(|output| output.status.success())
                .is_some_and(|output| {
                    let text = String::from_utf8_lossy(&output.stdout);
                    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
                    let saw_any = lines.clone().next().is_some();
                    saw_any && lines.all(|line| line.trim().starts_with('-'))
                });
            linear && patches
        }
        BranchProof::None => true,
    }
}

fn execute_branch_delete(
    repo: &Path,
    branch: &str,
    expected_oid: &str,
    proof: &BranchProof,
    opts: ReapOpts,
) -> Outcome {
    let full_ref = format!("refs/heads/{branch}");
    let Some(current) = git_output(repo, &["rev-parse", "--verify", &full_ref]) else {
        return Outcome::Failed("could not recheck branch identity".into());
    };
    if !current.status.success() || String::from_utf8_lossy(&current.stdout).trim() != expected_oid
    {
        return Outcome::Failed("branch changed after the scan; rescan required".into());
    }
    if !branch_proof_holds(repo, expected_oid, proof) {
        return Outcome::Failed(
            "branch recoverability proof changed after the scan; rescan required".into(),
        );
    }
    if opts.dry_run {
        return Outcome::Done;
    }

    // Verify the proof-bearing ref and delete the branch under one ref
    // transaction. Neither the branch nor the ref that justifies its risk grade
    // can move between proof and deletion.
    let mut transaction = String::from("start\n");
    match proof {
        BranchProof::ContainsRef {
            reference,
            expected_tip,
        }
        | BranchProof::PatchesIn {
            reference,
            expected_tip,
        } => {
            let _ = writeln!(transaction, "verify {reference} {expected_tip}");
        }
        BranchProof::None => {}
    }
    let _ = write!(
        transaction,
        "delete {full_ref} {expected_oid}\nprepare\ncommit\n"
    );
    let child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["update-ref", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let Ok(mut child) = child else {
        return Outcome::Failed("could not start atomic branch transaction".into());
    };
    let wrote = child
        .stdin
        .as_mut()
        .is_some_and(|stdin| stdin.write_all(transaction.as_bytes()).is_ok());
    if !wrote {
        return Outcome::Failed("could not write atomic branch transaction".into());
    }
    let deleted = child.wait_with_output();
    let Ok(deleted) = deleted else {
        return Outcome::Failed("could not finish atomic branch transaction".into());
    };
    if !deleted.status.success() {
        return Outcome::Failed(
            String::from_utf8_lossy(&deleted.stderr)
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("branch or recoverability ref changed during deletion")
                .to_string(),
        );
    }
    // Match `git branch -d/-D` housekeeping. Failure only leaves harmless
    // branch-specific configuration; the ref deletion itself already succeeded.
    let section = format!("branch.{branch}");
    let _ = git_output(repo, &["config", "--remove-section", &section]);
    Outcome::Done
}

#[expect(
    clippy::too_many_lines,
    reason = "the action dispatcher keeps every destructive path behind one audited safety boundary"
)]
fn execute(action: &Action, opts: ReapOpts) -> Outcome {
    match action {
        Action::None => Outcome::Failed("item is informational and cannot be reaped".into()),
        Action::Remove(path) => {
            if is_forbidden(path) {
                return Outcome::Failed("refused: path is too broad to delete".into());
            }
            // Checked here rather than at scan time on purpose: a link can be
            // introduced between the scan and the confirmation, and this is the
            // last moment before the filesystem is touched.
            if resolves_into_forbidden(path) {
                return Outcome::Failed(
                    "refused: path resolves through a link into a protected location".into(),
                );
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
        Action::GitBranchDelete {
            repo,
            branch,
            expected_oid,
            proof,
            ..
        } => execute_branch_delete(repo, branch, expected_oid, proof, opts),
        Action::GitWorktreeRemove {
            repo,
            path,
            expected_head,
            expected_status,
            require_surviving_ref,
            force,
        } => {
            if !path.exists() {
                return Outcome::Skipped;
            }
            #[cfg(not(windows))]
            {
                // A final, bounded activity check. `+d` examines only this
                // directory rather than recursively walking a potentially huge
                // worktree; agent processes normally keep the worktree root as
                // their cwd. Absence of lsof is not treated as proof either way,
                // so HEAD/status revalidation still remains the hard guard.
                if let Ok(output) = Command::new("lsof")
                    .args(["-n", "-P", "-a", "-d", "cwd", "+d"])
                    .arg(path)
                    .output()
                    && output.status.success()
                    && String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .nth(1)
                        .is_some()
                {
                    return Outcome::Failed(
                        "a running process is using this worktree; try again after it exits".into(),
                    );
                }
            }
            let inspect = |args: &[&str]| -> Result<String, String> {
                let output = Command::new("git")
                    .arg("-C")
                    .arg(path)
                    .args(args)
                    .output()
                    .map_err(|error| error.to_string())?;
                if !output.status.success() {
                    return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
                }
                Ok(String::from_utf8_lossy(&output.stdout).into_owned())
            };
            let current_head = match inspect(&["rev-parse", "HEAD"]) {
                Ok(head) => head.trim().to_string(),
                Err(error) => return Outcome::Failed(format!("could not recheck HEAD: {error}")),
            };
            if &current_head != expected_head {
                return Outcome::Failed(
                    "worktree HEAD changed after the scan; rescan required".into(),
                );
            }
            let current_status = match inspect(&[
                "status",
                "--porcelain=v1",
                "--untracked-files=all",
                "--ignored=matching",
            ]) {
                Ok(status) => status,
                Err(error) => return Outcome::Failed(format!("could not recheck files: {error}")),
            };
            if &current_status != expected_status {
                return Outcome::Failed(
                    "worktree files changed after the scan; rescan required".into(),
                );
            }
            // Keep this immediately before the destructive command. A status
            // walk can take long enough for another process to move a ref.
            if *require_surviving_ref {
                let refs = Command::new("git")
                    .arg("-C")
                    .arg(repo)
                    .args([
                        "for-each-ref",
                        "--format=%(refname)",
                        "--contains",
                        &current_head,
                    ])
                    .output();
                match refs {
                    Ok(output)
                        if output.status.success()
                            && String::from_utf8_lossy(&output.stdout)
                                .lines()
                                .any(|line| !line.trim().is_empty()) => {}
                    Ok(_) => {
                        return Outcome::Failed(
                            "the ref that made detached HEAD recoverable no longer contains it; rescan required"
                                .into(),
                        );
                    }
                    Err(error) => {
                        return Outcome::Failed(format!(
                            "could not recheck detached HEAD reachability: {error}"
                        ));
                    }
                }
            }
            if opts.dry_run {
                return Outcome::Done;
            }
            let mut command = Command::new("git");
            command.arg("-C").arg(repo).args(["worktree", "remove"]);
            if *force {
                command.arg("--force");
            }
            command.arg(path);
            match command.output() {
                Ok(output) if output.status.success() => Outcome::Done,
                Ok(output) => Outcome::Failed(
                    String::from_utf8_lossy(&output.stderr)
                        .lines()
                        .find(|line| !line.trim().is_empty())
                        .unwrap_or("git worktree remove failed")
                        .to_string(),
                ),
                Err(error) => Outcome::Failed(error.to_string()),
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

    /// An ordinary checkout, spelled the way the running platform spells one.
    ///
    /// A unix path is not absolute on Windows — it has no drive — so the
    /// literal would be refused there for the wrong reason entirely, and the
    /// specification would pass while proving nothing.
    fn an_ordinary_target() -> &'static str {
        if cfg!(windows) {
            r"C:\Users\someone\code\app\node_modules"
        } else {
            "/Users/someone/code/app/node_modules"
        }
    }

    /// A directory that exists on this platform and must never be deleted.
    fn a_system_directory() -> &'static str {
        if cfg!(windows) { r"C:\Windows" } else { "/usr" }
    }

    #[test]
    fn allows_a_normal_nested_target() {
        assert!(!is_forbidden(Path::new(an_ordinary_target())));
    }

    /// The backstop covers what is *inside* a system directory, not only the
    /// directory itself. `/usr/lib/node_modules` is where a global npm install
    /// lands, so this is a path that really exists on real machines.
    #[test]
    #[cfg(not(windows))]
    fn refuses_what_lives_inside_a_system_directory() {
        for p in [
            "/usr/lib/node_modules",
            "/etc/ssh",
            "/Library/Caches/com.apple.something",
            "/System/Library/Frameworks",
        ] {
            assert!(is_forbidden(Path::new(p)), "{p} should be refused");
        }
    }

    /// The roots whose children are ordinary must keep letting those children
    /// through, or reap refuses the only places it ever looks.
    #[test]
    #[cfg(not(windows))]
    fn still_allows_the_places_reap_actually_looks() {
        for p in [
            "/home/someone/.cache/pip",
            "/root/.cache/go-build",
            "/opt/homebrew/Caskroom",
            // macOS spells `$TMPDIR` this way.
            "/var/folders/xy/T/build",
            "/usrlocal/share",
        ] {
            assert!(!is_forbidden(Path::new(p)), "{p} should be allowed");
        }
    }

    /// A link is resolved before the refusal is decided.
    ///
    /// Only ancestors need this. The leaf is never followed: `remove_dir_all`
    /// refuses a symlink and the fallback unlinks the link itself, so what it
    /// points at is never at risk.
    #[cfg(unix)]
    mod through_a_link {
        use super::*;

        #[test]
        fn refuses_a_target_whose_parent_leads_into_a_system_directory() {
            let root = tmp("link-into-system");
            let link = root.join("cache");
            std::os::unix::fs::symlink("/usr/lib", &link).unwrap();
            let target = link.join("node_modules");

            assert!(
                !is_forbidden(&target),
                "the literal path is unremarkable, which is the point"
            );
            assert!(resolves_into_forbidden(&target));
        }

        /// Deny-only: resolution may add a refusal and must never invent one
        /// for a target that simply lives behind a link.
        #[test]
        fn allows_a_target_whose_parent_leads_somewhere_ordinary() {
            let root = tmp("link-into-ordinary");
            let real = root.join("elsewhere/deps");
            std::fs::create_dir_all(&real).unwrap();
            let link = root.join("cache");
            std::os::unix::fs::symlink(&real, &link).unwrap();

            assert!(!resolves_into_forbidden(&link.join("node_modules")));
        }

        /// A path that does not exist yet cannot be resolved, and that is not
        /// evidence of anything. `execute` reports it as nothing to do.
        #[test]
        fn says_nothing_about_a_parent_that_does_not_exist() {
            let root = tmp("link-absent");
            assert!(!resolves_into_forbidden(&root.join("gone/deps")));
        }

        #[test]
        fn refuses_rather_than_deletes_what_the_link_points_at() {
            let root = tmp("link-refusal");
            let real = root.join("precious");
            std::fs::create_dir_all(&real).unwrap();
            std::fs::write(real.join("work.txt"), b"the only copy").unwrap();

            // A link standing in for a redirected cache root, pointed at a
            // directory the depth rule would otherwise wave through.
            let link = root.join("cache");
            std::os::unix::fs::symlink("/usr/lib", &link).unwrap();

            let out = run(
                vec![candidate("through a link", 100, &link.join("node_modules"))],
                plain(false),
            );
            assert_eq!(out.len(), 1);
            assert!(!out[0].ok);
            assert_eq!(out[0].freed, 0);
            assert!(
                out[0].error.as_ref().unwrap().contains("resolves"),
                "error was: {:?}",
                out[0].error
            );
            assert!(real.join("work.txt").exists());
        }
    }

    #[test]
    fn forbidden_paths_fail_rather_than_delete() {
        let out = run(vec![candidate("root", 100, Path::new("/"))], plain(false));
        assert_eq!(out.len(), 1);
        assert!(!out[0].ok, "deleting / must not report success");
        assert_eq!(out[0].freed, 0);
        assert!(out[0].error.as_ref().unwrap().contains("too broad"));
        assert!(
            Path::new(a_system_directory()).exists(),
            "filesystem must be untouched"
        );
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

    /// Trashing must never quietly become deleting.
    ///
    /// Windows hands the path to the shell, which recycles it and does not say
    /// where it put it, so there is nothing for reap to read back — which is
    /// why the recoverability specification below is unix-only. What holds on
    /// every platform is this: either it was trashed and the original is gone,
    /// or it failed and the original is still there. The outcome that must not
    /// exist is a reported success over a path that was unlinked instead.
    #[cfg(windows)]
    #[test]
    fn trashing_either_takes_it_or_leaves_it_where_it_was() {
        let root = tmp("trash-win");
        let target = root.join(format!("reap-win-{}", std::process::id()));
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("keep.txt"), b"recoverable").unwrap();

        let out = run(
            vec![candidate("nm", 4096, &target)],
            ReapOpts {
                dry_run: false,
                trash: true,
            },
        );

        if out[0].ok {
            assert!(!target.exists(), "it reported success and left the path");
            // The shell keeps its own index of the bin, so reap is not in a
            // position to offer to empty what it just put there.
            assert!(out[0].trashed.is_none(), "nothing to hand back on Windows");
        } else {
            assert!(
                target.exists(),
                "it failed and deleted the path anyway, which is the one \
                 outcome --trash exists to rule out"
            );
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
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
            100,
            Risk::Caution,
            Action::Run {
                program: "true".into(),
                args: vec![],
                cwd: None,
            },
        )
        .with_footprint(root.join("wt"));
        let out = run(vec![candidate("nested", 50, &nested), wt], plain(true));
        assert_eq!(out[0].label, "worktree");
        assert_eq!(out.iter().map(|report| report.freed).sum::<u64>(), 100);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn branch_and_recoverability_ref_are_verified_in_one_delete_transaction() {
        let repo = tmp("branch-proof-transaction");
        let run_git = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["-c", "user.name=Spec", "-c", "user.email=spec@example.com"])
                .args(args)
                .output()
                .expect("git runs");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).into_owned()
        };
        run_git(&["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("base.txt"), "base\n").unwrap();
        run_git(&["add", "base.txt"]);
        run_git(&["commit", "-q", "-m", "base"]);
        run_git(&["checkout", "-q", "-b", "feature"]);
        std::fs::write(repo.join("feature.txt"), "feature\n").unwrap();
        run_git(&["add", "feature.txt"]);
        run_git(&["commit", "-q", "-m", "feature"]);
        let branch_oid = run_git(&["rev-parse", "feature"]).trim().to_string();
        run_git(&["checkout", "-q", "main"]);
        run_git(&["merge", "-q", "--no-ff", "-m", "merge feature", "feature"]);
        let main_oid = run_git(&["rev-parse", "main"]).trim().to_string();

        let outcome = execute(
            &Action::GitBranchDelete {
                repo: repo.clone(),
                branch: "feature".into(),
                expected_oid: branch_oid,
                proof: BranchProof::ContainsRef {
                    reference: "refs/heads/main".into(),
                    expected_tip: main_oid,
                },
                force: false,
            },
            plain(false),
        );
        assert!(matches!(outcome, Outcome::Done));
        let deleted = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["rev-parse", "--verify", "refs/heads/feature"])
            .output()
            .unwrap();
        assert!(!deleted.status.success());
        std::fs::remove_dir_all(repo).ok();
    }

    #[test]
    fn branch_deletion_aborts_if_the_branch_advanced_after_the_scan() {
        let repo = tmp("branch-race");
        let run_git = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["-c", "user.name=Spec", "-c", "user.email=spec@example.com"])
                .args(args)
                .output()
                .expect("git runs");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).into_owned()
        };
        run_git(&["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("base.txt"), "base\n").unwrap();
        run_git(&["add", "base.txt"]);
        run_git(&["commit", "-q", "-m", "base"]);
        run_git(&["checkout", "-q", "-b", "feature"]);
        std::fs::write(repo.join("feature.txt"), "one\n").unwrap();
        run_git(&["add", "feature.txt"]);
        run_git(&["commit", "-q", "-m", "feature one"]);
        let scanned = run_git(&["rev-parse", "feature"]).trim().to_string();
        std::fs::write(repo.join("feature.txt"), "two\n").unwrap();
        run_git(&["add", "feature.txt"]);
        run_git(&["commit", "-q", "-m", "feature two"]);
        let advanced = run_git(&["rev-parse", "feature"]).trim().to_string();
        run_git(&["checkout", "-q", "main"]);

        let outcome = execute(
            &Action::GitBranchDelete {
                repo: repo.clone(),
                branch: "feature".into(),
                expected_oid: scanned,
                proof: BranchProof::None,
                force: true,
            },
            plain(false),
        );
        assert!(matches!(outcome, Outcome::Failed(_)));
        assert_eq!(run_git(&["rev-parse", "feature"]).trim(), advanced);
        std::fs::remove_dir_all(repo).ok();
    }

    #[test]
    fn detached_worktree_removal_rechecks_the_ref_that_made_it_recoverable() {
        let repo = tmp("detached-ref-race");
        let worktree = repo.with_extension("detached");
        let run_git = |dir: &Path, args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["-c", "user.name=Spec", "-c", "user.email=spec@example.com"])
                .args(args)
                .output()
                .expect("git runs");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).into_owned()
        };
        run_git(&repo, &["init", "-q"]);
        std::fs::write(repo.join("tracked.txt"), "one\n").unwrap();
        run_git(&repo, &["add", "tracked.txt"]);
        run_git(&repo, &["commit", "-q", "-m", "initial"]);
        run_git(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                worktree.to_str().unwrap(),
                "HEAD",
            ],
        );
        let head = run_git(&worktree, &["rev-parse", "HEAD"])
            .trim()
            .to_string();
        run_git(&repo, &["update-ref", "-d", "refs/heads/master"]);
        run_git(&repo, &["update-ref", "-d", "refs/heads/main"]);

        let outcome = execute(
            &Action::GitWorktreeRemove {
                repo: repo.clone(),
                path: worktree.clone(),
                expected_head: head,
                expected_status: String::new(),
                require_surviving_ref: true,
                force: false,
            },
            plain(false),
        );
        assert!(matches!(outcome, Outcome::Failed(_)));
        assert!(worktree.exists());
        std::fs::remove_dir_all(&worktree).ok();
        std::fs::remove_dir_all(repo).ok();
    }

    #[test]
    fn worktree_removal_aborts_if_files_changed_after_the_scan() {
        let repo = tmp("worktree-race");
        let run_git = |args: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["-c", "user.name=Spec", "-c", "user.email=spec@example.com"])
                .args(args)
                .status()
                .unwrap();
            assert!(status.success());
        };
        run_git(&["init", "-q"]);
        std::fs::write(repo.join("tracked.txt"), "one\n").unwrap();
        run_git(&["add", "tracked.txt"]);
        run_git(&["commit", "-q", "-m", "initial"]);
        let head = String::from_utf8_lossy(
            &Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();
        std::fs::write(repo.join("tracked.txt"), "changed after scan\n").unwrap();

        let outcome = execute(
            &Action::GitWorktreeRemove {
                repo: repo.clone(),
                path: repo.clone(),
                expected_head: head,
                expected_status: String::new(),
                require_surviving_ref: false,
                force: true,
            },
            plain(false),
        );
        assert!(matches!(outcome, Outcome::Failed(_)));
        assert!(repo.exists());
        std::fs::remove_dir_all(repo).ok();
    }
}
