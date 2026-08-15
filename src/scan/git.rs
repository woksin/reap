use super::ScanOpts;
use crate::model::{Action, Candidate, Category, Risk, ScanEvent};
use crate::util::{days_since, tilde};
use rayon::prelude::*;
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
    // Evaluating a repository means spawning a `git` process per branch, which
    // is what dominates a scan. Repositories are independent, so they run
    // concurrently rather than one after another.
    repos.par_iter().for_each_with(tx.clone(), |tx, repo| {
        let name = repo
            .file_name()
            .map_or_else(|| tilde(repo), |n| n.to_string_lossy().into_owned());
        let _ = tx.send(ScanEvent::Status(format!("git: {name}")));

        branches(repo, &name, opts, tx);
        worktrees(repo, &name, opts, tx);
        stashes(repo, &name, opts, tx);
        housekeeping(repo, &name, opts, tx);
    });
}

/// The branch everything else is measured against.
fn default_branch(repo: &Path) -> String {
    if let Some(head) = git(
        repo,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ) && let Some(b) = head.trim().strip_prefix("origin/")
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
        .map_or_else(|| "main".into(), |s| s.trim().to_string())
}

fn branches(repo: &Path, repo_name: &str, opts: &ScanOpts, tx: &Sender<ScanEvent>) {
    let default = default_branch(repo);
    let current = git(repo, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    // Compare against the remote tip when we have one — a local default branch
    // that has drifted behind origin would under-report merged work.
    let merge_base = if git(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("origin/{default}"),
        ],
    )
    .is_some()
    {
        format!("origin/{default}")
    } else {
        default.clone()
    };

    let merged: Vec<String> = git(
        repo,
        &[
            "branch",
            "--merged",
            &merge_base,
            "--format=%(refname:short)",
        ],
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
        let Verdict {
            group,
            detail,
            risk,
            force,
        } = grade(
            survives(repo, &branch, &merge_base, is_merged),
            &merge_base,
            &upstream,
            is_gone,
        );

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
        super::emit(tx, opts, cand);
    }
}

/// How a branch is filed, described and graded once we know what survives it.
struct Verdict {
    /// Sub-bucket in the sidebar, e.g. "merged branches".
    group: &'static str,
    /// The reason, in the words shown next to the branch.
    detail: String,
    risk: Risk,
    /// Whether git needs `-D` rather than `-d` to let go of it.
    force: bool,
}

/// Put what survives deleting a branch into words, and grade the risk of it.
///
/// Split out from the walk so the wording and the grading of a branch can be
/// read — and argued about — without the listing and filtering around it.
fn grade(survives: Survives, merge_base: &str, upstream: &str, is_gone: bool) -> Verdict {
    match survives {
        Survives::Merged => Verdict {
            group: "merged branches",
            detail: format!("already merged into {merge_base}"),
            risk: Risk::Safe,
            force: false,
        },
        Survives::PatchesUpstream { commits } => Verdict {
            group: "squash-merged branches",
            detail: format!(
                "all {commits} commits are already in {merge_base} by content — squash- or rebase-merged"
            ),
            risk: Risk::Safe,
            force: true,
        },
        Survives::OnRemote { commits, remote } => Verdict {
            group: "pushed branches",
            detail: format!(
                "{commits} unmerged commits, all pushed to {remote} — recoverable from the remote"
            ),
            risk: Risk::Caution,
            force: true,
        },
        Survives::LocalOnly { commits } => Verdict {
            group: "unpushed branches",
            // A "gone" upstream no longer exists, so naming it as merely
            // lacking the commits would misdescribe what happened.
            detail: if is_gone {
                format!("{commits} commits exist only here — upstream {upstream} was deleted")
            } else if upstream.is_empty() {
                format!("{commits} commits exist only here — never pushed anywhere")
            } else {
                format!("{commits} commits exist only here — {upstream} does not have them")
            },
            risk: Risk::Danger,
            force: true,
        },
        Survives::Unknown { reason } => Verdict {
            group: "unverified branches",
            detail: format!("could not prove this branch recoverable — {reason}"),
            risk: Risk::Danger,
            force: true,
        },
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
    /// Git could not answer one of the questions needed for a safe verdict.
    Unknown { reason: &'static str },
}

fn rev_count(repo: &Path, args: &[&str]) -> Option<u64> {
    git(repo, args).and_then(|s| s.trim().parse().ok())
}

fn survives(repo: &Path, branch: &str, merge_base: &str, is_merged: bool) -> Survives {
    if is_merged {
        return Survives::Merged;
    }

    let Some(ahead) = rev_count(
        repo,
        &["rev-list", "--count", &format!("{merge_base}..{branch}")],
    ) else {
        return Survives::Unknown {
            reason: "git could not count its commits",
        };
    };
    if ahead == 0 {
        return Survives::Merged;
    }

    // `git cherry` compares patch ids, so it sees through the rewritten SHAs a
    // squash or rebase merge produces. It does not emit merge commits, however:
    // a conflict resolution or any other content introduced by a merge would be
    // invisible and could make a branch with unique work look safe. Only use the
    // patch-id shortcut for a linear branch.
    let range = format!("{merge_base}..{branch}");
    let Some(merge_commits) = rev_count(repo, &["rev-list", "--count", "--merges", &range]) else {
        return Survives::Unknown {
            reason: "git could not inspect its merge history",
        };
    };
    if merge_commits == 0
        && let Some(cherry) = git(repo, &["cherry", merge_base, branch])
    {
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
    let Some(unpushed) = rev_count(repo, &["rev-list", "--count", branch, "--not", "--remotes"])
    else {
        return Survives::Unknown {
            reason: "git could not verify its remote-tracking refs",
        };
    };
    if unpushed == 0 {
        let remote = git(
            repo,
            &[
                "branch",
                "-r",
                "--contains",
                branch,
                "--format=%(refname:short)",
            ],
        )
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

/// Read `git worktree list --porcelain`.
///
/// Each record opens with a `worktree` line and the attributes that follow
/// belong to it, so anything before the first one is ignored rather than
/// guessed at.
fn parse_worktree_list(out: &str) -> Vec<Worktree> {
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
    trees
}

/// Count files Git would otherwise hide from a worktree-cleanliness check.
fn worktree_files(path: &Path) -> Option<(usize, usize)> {
    git(
        path,
        &[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--ignored=matching",
        ],
    )
    .map(|status| {
        let mut uncommitted = 0;
        let mut ignored = 0;
        for line in status.lines().filter(|line| !line.trim().is_empty()) {
            if line.starts_with("!! ") {
                ignored += 1;
            } else {
                uncommitted += 1;
            }
        }
        (uncommitted, ignored)
    })
}

fn worktree_verdict(
    branch: &str,
    files: Option<(usize, usize)>,
    unpushed: Option<u64>,
) -> (Risk, String) {
    let (risk, detail) = match (files, unpushed) {
        (None, _) => (
            Risk::Danger,
            "git could not inspect files in this worktree".into(),
        ),
        (_, None) => (
            Risk::Danger,
            "git could not verify where its commits survive".into(),
        ),
        (Some((0, 0)), Some(0)) => (
            Risk::Caution,
            "clean, every commit is on a remote — safe to prune".into(),
        ),
        (Some((0, 0)), Some(n)) => (
            Risk::Danger,
            format!("clean, but {n} commits exist only here"),
        ),
        (Some((d, 0)), Some(0)) => (
            Risk::Danger,
            format!("{d} uncommitted files, commits are all pushed"),
        ),
        (Some((d, 0)), Some(n)) => (
            Risk::Danger,
            format!("{d} uncommitted files and {n} unpushed commits"),
        ),
        (Some((0, i)), Some(0)) => (
            Risk::Danger,
            format!("{i} ignored files may exist only here; commits are all pushed"),
        ),
        (Some((0, i)), Some(n)) => (
            Risk::Danger,
            format!("{i} ignored files and {n} unpushed commits"),
        ),
        (Some((d, i)), Some(0)) => (
            Risk::Danger,
            format!("{d} uncommitted and {i} ignored files; commits are all pushed"),
        ),
        (Some((d, i)), Some(n)) => (
            Risk::Danger,
            format!("{d} uncommitted files, {i} ignored files and {n} unpushed commits"),
        ),
    };
    (risk, format!("[{branch}] {detail}"))
}

fn worktrees(repo: &Path, repo_name: &str, opts: &ScanOpts, tx: &Sender<ScanEvent>) {
    let Some(out) = git(repo, &["worktree", "list", "--porcelain"]) else {
        return;
    };

    let trees = parse_worktree_list(&out);

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
            super::emit(tx, opts, cand);
            continue;
        }

        let age = crate::util::age_days(&wt.path).unwrap_or(0);
        if age < opts.stale_days {
            continue;
        }
        let size = opts.cache.size_of(&wt.path);

        // Removing a worktree deletes its working directory outright, so both
        // uncommitted files and commits held only here are lost with it.
        // Ignored files are still files. `git status --porcelain` hides them by
        // default, while `git worktree remove --force` deletes them. Ask for
        // matching ignored entries explicitly so an ignored database, secret or
        // export cannot be described as an empty checkout.
        let files = worktree_files(&wt.path);
        let unpushed = match &wt.branch {
            Some(b) => rev_count(repo, &["rev-list", "--count", b, "--not", "--remotes"]),
            // A detached HEAD is unreachable from any ref once the worktree goes.
            None => rev_count(
                &wt.path,
                &["rev-list", "--count", "HEAD", "--not", "--remotes"],
            ),
        };

        let (risk, detail) = worktree_verdict(&branch, files, unpushed);

        // A clean worktree should still get Git's own refusal if the scan raced
        // with a new file. `--force` is reserved for an item already behind the
        // irreversible confirmation.
        let mut args = vec!["worktree".into(), "remove".into()];
        if risk == Risk::Danger {
            args.push("--force".into());
        }
        args.push(wt.path.display().to_string());

        let cand = Candidate::new(
            Category::Git,
            "stale worktrees",
            label,
            detail,
            size,
            risk,
            Action::Run {
                program: "git".into(),
                args,
                cwd: Some(repo.to_path_buf()),
            },
        )
        .with_age(Some(age));
        super::emit(tx, opts, cand);
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
        super::emit(tx, opts, cand);
    }
}

/// Loose objects and cruft that `git gc` would compact away.
fn housekeeping(repo: &Path, repo_name: &str, opts: &ScanOpts, tx: &Sender<ScanEvent>) {
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
    super::emit(tx, opts, cand);
}
