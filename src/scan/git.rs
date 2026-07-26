use super::ScanOpts;
use crate::model::{Action, Candidate, Category, Risk, ScanEvent};
use crate::util::{days_since, tilde};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::Sender;

fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn scan(repos: &[PathBuf], opts: &ScanOpts, tx: &Sender<ScanEvent>) {
    for repo in repos {
        let name = repo
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| tilde(repo));
        let _ = tx.send(ScanEvent::Status(format!("git: {name}")));

        branches(repo, &name, opts, tx);
        worktrees(repo, &name, opts, tx);
        stashes(repo, &name, opts, tx);
        housekeeping(repo, &name, tx);
    }
}

/// The branch everything else is measured against.
fn default_branch(repo: &Path) -> String {
    if let Some(head) = git(repo, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        && let Some(b) = head.trim().strip_prefix("origin/")
        && !b.is_empty()
    {
        return b.to_string();
    }
    for cand in ["main", "master", "develop", "trunk"] {
        if git(repo, &["rev-parse", "--verify", "--quiet", cand]).is_some() {
            return cand.to_string();
        }
    }
    git(repo, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "main".into())
}

fn branches(repo: &Path, repo_name: &str, opts: &ScanOpts, tx: &Sender<ScanEvent>) {
    let default = default_branch(repo);
    let current = git(repo, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    // Compare against the remote tip when we have one — a local default branch
    // that has drifted behind origin would under-report merged work.
    let merge_base = if git(repo, &["rev-parse", "--verify", "--quiet", &format!("origin/{default}")])
        .is_some()
    {
        format!("origin/{default}")
    } else {
        default.clone()
    };

    let merged: Vec<String> = git(
        repo,
        &["branch", "--merged", &merge_base, "--format=%(refname:short)"],
    )
    .unwrap_or_default()
    .lines()
    .map(|l| l.trim().to_string())
    .filter(|l| !l.is_empty())
    .collect();

    // Branches checked out in a linked worktree cannot be deleted, and deleting
    // them out from under a worktree would be hostile anyway.
    let in_worktree = worktree_branches(repo);

    let listing = git(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname:short)%09%(upstream:short)%09%(upstream:track)%09%(committerdate:unix)",
            "refs/heads/",
        ],
    )
    .unwrap_or_default();

    for line in listing.lines() {
        let mut f = line.split('\t');
        let branch = f.next().unwrap_or("").trim().to_string();
        let upstream = f.next().unwrap_or("").trim().to_string();
        let track = f.next().unwrap_or("").trim().to_string();
        let committed: u64 = f.next().unwrap_or("0").trim().parse().unwrap_or(0);

        if branch.is_empty()
            || branch == default
            || branch == current
            || in_worktree.contains(&branch)
        {
            continue;
        }

        let age = days_since(committed);
        let is_merged = merged.iter().any(|m| m == &branch);
        let is_gone = track.contains("gone");

        // Only branches that are merged, orphaned or stale are worth offering.
        if !is_merged && !is_gone && age < opts.stale_days {
            continue;
        }

        // Work out what actually survives deleting this branch, rather than
        // inferring it from the branch's name or its tracking status.
        let (group, detail, risk, force) = match survives(repo, &branch, &merge_base, is_merged) {
            Survives::Merged => (
                "merged branches",
                format!("already merged into {merge_base}"),
                Risk::Safe,
                false,
            ),
            Survives::PatchesUpstream { commits } => (
                "squash-merged branches",
                format!(
                    "all {commits} commits are already in {merge_base} by content — squash- or rebase-merged"
                ),
                Risk::Safe,
                true,
            ),
            Survives::OnRemote { commits, remote } => (
                "pushed branches",
                format!("{commits} unmerged commits, all pushed to {remote} — recoverable from the remote"),
                Risk::Caution,
                true,
            ),
            Survives::LocalOnly { commits } => (
                "unpushed branches",
                // A "gone" upstream no longer exists, so naming it as merely
                // lacking the commits would misdescribe what happened.
                if is_gone {
                    format!("{commits} commits exist only here — upstream {upstream} was deleted")
                } else if upstream.is_empty() {
                    format!("{commits} commits exist only here — never pushed anywhere")
                } else {
                    format!("{commits} commits exist only here — {upstream} does not have them")
                },
                Risk::Danger,
                true,
            ),
        };

        let flag = if force { "-D" } else { "-d" };
        let cand = Candidate::new(
            Category::Git,
            group,
            format!("{repo_name}/{branch}"),
            detail,
            0,
            risk,
            Action::Run {
                program: "git".into(),
                args: vec!["branch".into(), flag.into(), branch.clone()],
                cwd: Some(repo.to_path_buf()),
            },
        )
        .with_age(Some(age));
        let _ = tx.send(ScanEvent::Found(Box::new(cand)));
    }
}

/// What still exists after a branch is deleted.
///
/// This is the question that decides whether pruning is safe, and it cannot be
/// answered from the branch name or its tracking status — a squash-merged PR
/// leaves a branch that looks unmerged to `--merged` but whose every line of
/// work is already in the integration branch.
#[derive(Debug, PartialEq, Eq)]
enum Survives {
    /// Reachable from the integration branch: an ordinary merge.
    Merged,
    /// Not an ancestor, but every patch is already upstream by content.
    PatchesUpstream { commits: u64 },
    /// Unmerged, but every commit is on a remote and can be fetched back.
    OnRemote { commits: u64, remote: String },
    /// Commits that exist in this clone and nowhere else.
    LocalOnly { commits: u64 },
}

fn rev_count(repo: &Path, args: &[&str]) -> u64 {
    git(repo, args)
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn survives(repo: &Path, branch: &str, merge_base: &str, is_merged: bool) -> Survives {
    if is_merged {
        return Survives::Merged;
    }

    let ahead = rev_count(
        repo,
        &["rev-list", "--count", &format!("{merge_base}..{branch}")],
    );
    if ahead == 0 {
        return Survives::Merged;
    }

    // `git cherry` compares patch ids, so it sees through the rewritten SHAs a
    // squash or rebase merge produces. A `-` prefix means the patch is already
    // upstream; a `+` means it is not.
    if let Some(cherry) = git(repo, &["cherry", merge_base, branch]) {
        let mut saw_any = false;
        let mut all_upstream = true;
        for line in cherry.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            saw_any = true;
            if line.starts_with('+') {
                all_upstream = false;
                break;
            }
        }
        if saw_any && all_upstream {
            return Survives::PatchesUpstream { commits: ahead };
        }
    }

    // Commits on this branch that no remote-tracking ref can reach.
    let unpushed = rev_count(repo, &["rev-list", "--count", branch, "--not", "--remotes"]);
    if unpushed == 0 {
        let remote = git(repo, &["branch", "-r", "--contains", branch, "--format=%(refname:short)"])
            .unwrap_or_default()
            .lines()
            .next()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "a remote".into());
        return Survives::OnRemote {
            commits: ahead,
            remote,
        };
    }

    Survives::LocalOnly { commits: unpushed }
}

/// Branches currently checked out in any worktree of this repository.
fn worktree_branches(repo: &Path) -> Vec<String> {
    git(repo, &["worktree", "list", "--porcelain"])
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.strip_prefix("branch refs/heads/"))
        .map(|b| b.trim().to_string())
        .collect()
}

#[derive(Default)]
struct Worktree {
    path: PathBuf,
    branch: Option<String>,
    prunable: Option<String>,
    locked: bool,
    bare: bool,
}

fn worktrees(repo: &Path, repo_name: &str, opts: &ScanOpts, tx: &Sender<ScanEvent>) {
    let Some(out) = git(repo, &["worktree", "list", "--porcelain"]) else {
        return;
    };

    let mut trees: Vec<Worktree> = Vec::new();
    for line in out.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            trees.push(Worktree {
                path: PathBuf::from(p.trim()),
                ..Default::default()
            });
        } else if let Some(cur) = trees.last_mut() {
            if let Some(b) = line.strip_prefix("branch refs/heads/") {
                cur.branch = Some(b.trim().to_string());
            } else if let Some(reason) = line.strip_prefix("prunable") {
                cur.prunable = Some(reason.trim().trim_start_matches(' ').to_string());
            } else if line.starts_with("locked") {
                cur.locked = true;
            } else if line.starts_with("bare") {
                cur.bare = true;
            }
        }
    }

    // The first entry is the main working tree; it is the repository itself.
    for wt in trees.iter().skip(1) {
        // A locked worktree is an explicit "leave this alone" from the user.
        if wt.locked || wt.bare {
            continue;
        }

        let exists = wt.path.exists();
        let label = format!("{repo_name}: {}", tilde(&wt.path));
        let branch = wt.branch.clone().unwrap_or_else(|| "detached".into());

        if !exists || wt.prunable.is_some() {
            let reason = wt
                .prunable
                .clone()
                .filter(|r| !r.is_empty())
                .unwrap_or_else(|| "working tree directory is gone".into());
            let cand = Candidate::new(
                Category::Git,
                "prunable worktrees",
                label,
                format!("[{branch}] {reason}"),
                0,
                Risk::Safe,
                Action::Run {
                    program: "git".into(),
                    args: vec!["worktree".into(), "prune".into()],
                    cwd: Some(repo.to_path_buf()),
                },
            );
            let _ = tx.send(ScanEvent::Found(Box::new(cand)));
            continue;
        }

        let age = crate::util::age_days(&wt.path).unwrap_or(0);
        if age < opts.stale_days {
            continue;
        }
        let size = opts.cache.size_of(&wt.path);

        // Removing a worktree deletes its working directory outright, so both
        // uncommitted files and commits held only here are lost with it.
        let dirty = git(&wt.path, &["status", "--porcelain"])
            .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
            .unwrap_or(0);
        let unpushed = match &wt.branch {
            Some(b) => rev_count(repo, &["rev-list", "--count", b, "--not", "--remotes"]),
            // A detached HEAD is unreachable from any ref once the worktree goes.
            None => rev_count(&wt.path, &["rev-list", "--count", "HEAD", "--not", "--remotes"]),
        };

        let (risk, detail) = match (dirty, unpushed) {
            (0, 0) => (
                Risk::Caution,
                format!("[{branch}] clean, every commit is on a remote — safe to prune"),
            ),
            (0, n) => (
                Risk::Danger,
                format!("[{branch}] clean, but {n} commits exist only here"),
            ),
            (d, 0) => (
                Risk::Danger,
                format!("[{branch}] {d} uncommitted files, commits are all pushed"),
            ),
            (d, n) => (
                Risk::Danger,
                format!("[{branch}] {d} uncommitted files and {n} unpushed commits"),
            ),
        };

        let cand = Candidate::new(
            Category::Git,
            "stale worktrees",
            label,
            detail,
            size,
            risk,
            Action::Run {
                program: "git".into(),
                args: vec![
                    "worktree".into(),
                    "remove".into(),
                    "--force".into(),
                    wt.path.display().to_string(),
                ],
                cwd: Some(repo.to_path_buf()),
            },
        )
        .with_age(Some(age));
        let _ = tx.send(ScanEvent::Found(Box::new(cand)));
    }
}

fn stashes(repo: &Path, repo_name: &str, opts: &ScanOpts, tx: &Sender<ScanEvent>) {
    let Some(out) = git(repo, &["stash", "list", "--format=%gd%x09%ct%x09%s"]) else {
        return;
    };
    for line in out.lines() {
        let mut f = line.split('\t');
        let reflog = f.next().unwrap_or("").trim().to_string();
        let ts: u64 = f.next().unwrap_or("0").trim().parse().unwrap_or(0);
        let subject = f.next().unwrap_or("").trim().to_string();
        if reflog.is_empty() {
            continue;
        }
        let age = days_since(ts);
        if age < opts.stale_days {
            continue;
        }
        // Stashes are dropped by index, so removing one shifts the rest. Always
        // target the ref by name and let the reaper run them one at a time.
        let cand = Candidate::new(
            Category::Git,
            "old stashes",
            format!("{repo_name}: {reflog}"),
            subject,
            0,
            Risk::Danger,
            Action::Run {
                program: "git".into(),
                args: vec!["stash".into(), "drop".into(), reflog.clone()],
                cwd: Some(repo.to_path_buf()),
            },
        )
        .with_age(Some(age));
        let _ = tx.send(ScanEvent::Found(Box::new(cand)));
    }
}

/// Loose objects and cruft that `git gc` would compact away.
fn housekeeping(repo: &Path, repo_name: &str, tx: &Sender<ScanEvent>) {
    let Some(out) = git(repo, &["count-objects", "-v"]) else {
        return;
    };
    let mut loose_count = 0u64;
    let mut loose_kib = 0u64;
    let mut garbage_kib = 0u64;
    for line in out.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let v: u64 = v.trim().parse().unwrap_or(0);
        match k.trim() {
            "count" => loose_count = v,
            "size" => loose_kib = v,
            "size-garbage" => garbage_kib = v,
            _ => {}
        }
    }

    let reclaimable = (loose_kib + garbage_kib) * 1024;
    if loose_count < 2000 && reclaimable < 50 * 1024 * 1024 {
        return;
    }

    // Deliberately not `--prune=now`: reap also offers to delete branches, and
    // pruning immediately would destroy the reflog that makes those recoverable.
    // The default two-week grace period keeps that safety net intact.
    let cand = Candidate::new(
        Category::Git,
        "repacking",
        format!("{repo_name}: git gc"),
        format!("{loose_count} loose objects to repack · keeps the reflog grace period"),
        reclaimable,
        Risk::Safe,
        Action::Run {
            program: "git".into(),
            args: vec!["gc".into(), "--quiet".into()],
            cwd: Some(repo.to_path_buf()),
        },
    );
    let _ = tx.send(ScanEvent::Found(Box::new(cand)));
}
