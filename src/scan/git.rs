use super::ScanOpts;
use crate::model::{Action, BranchProof, Candidate, Category, Eligibility, Risk, ScanEvent};
use crate::util::{days_since, tilde};
use rayon::prelude::*;
use std::collections::HashSet;
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

fn git_objects_dir(repo: &Path) -> Option<PathBuf> {
    let raw = git(repo, &["rev-parse", "--git-common-dir"])?;
    let path = PathBuf::from(raw.trim());
    let common = if path.is_absolute() {
        path
    } else {
        repo.join(path)
    };
    Some(
        std::fs::canonicalize(&common)
            .unwrap_or(common)
            .join("objects"),
    )
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
    orphaned_agent_worktrees(repos, opts, tx);
}

/// Agent tools sometimes leave a checkout directory after its Git worktree
/// registration is gone. It is still disk usage, but without the registration
/// reap cannot prove which command owns it or what survives removal, so it is
/// catalogued as protected rather than deleted as an ordinary directory.
fn orphaned_agent_worktrees(repos: &[PathBuf], opts: &ScanOpts, tx: &Sender<ScanEvent>) {
    let mut registered = HashSet::new();
    for repo in repos {
        if let Some(list) = git(repo, &["worktree", "list", "--porcelain"]) {
            registered.extend(
                parse_worktree_list(&list)
                    .into_iter()
                    .filter_map(|worktree| {
                        std::fs::canonicalize(&worktree.path)
                            .ok()
                            .or(Some(worktree.path))
                    }),
            );
        }
    }

    let mut found = Vec::new();
    for root in &opts.roots {
        collect_agent_worktrees(root, 0, opts.max_depth, opts, &mut found);
    }
    found.sort();
    found.dedup();

    for path in found {
        let canonical = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if registered.contains(&canonical) || !path.join(".git").exists() {
            continue;
        }
        let (size, newest) = opts.cache.measure(&path);
        if size < opts.min_size {
            continue;
        }
        let age = newest.map(days_since);
        let candidate = Candidate::new(
            Category::Git,
            "orphaned agent worktrees",
            tilde(&path),
            "checkout-like directory is not registered with its repository · inspect manually",
            size,
            Risk::Danger,
            Action::None,
        )
        .with_age(age)
        .with_footprint(path)
        .with_eligibility(Eligibility::Protected);
        super::emit(tx, opts, candidate);
    }
}

fn collect_agent_worktrees(
    dir: &Path,
    depth: usize,
    max_depth: usize,
    opts: &ScanOpts,
    out: &mut Vec<PathBuf>,
) {
    if depth > max_depth {
        return;
    }
    let is_store = dir.file_name().is_some_and(|name| name == ".worktrees")
        || (dir.file_name().is_some_and(|name| name == "worktrees")
            && dir
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| matches!(name.to_str(), Some(".claude" | ".codex" | ".pi"))));
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let path = entry.path();
        if is_store {
            out.push(path);
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !opts.rules.is_never_descend(&name) {
            collect_agent_worktrees(&path, depth + 1, max_depth, opts, out);
        }
    }
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

fn resolved_branch_proof(repo: &Path, reference: &str, patches: bool) -> Option<BranchProof> {
    let full = git(repo, &["rev-parse", "--symbolic-full-name", reference])?
        .trim()
        .to_string();
    let expected_tip = git(repo, &["rev-parse", "--verify", reference])?
        .trim()
        .to_string();
    if full.is_empty() || expected_tip.is_empty() {
        return None;
    }
    Some(if patches {
        BranchProof::PatchesIn {
            reference: full,
            expected_tip,
        }
    } else {
        BranchProof::ContainsRef {
            reference: full,
            expected_tip,
        }
    })
}

#[expect(
    clippy::too_many_lines,
    reason = "branch listing, recoverability grading, proof pinning and typed action creation form one evidence pipeline"
)]
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
            "--format=%(refname:short)%09%(upstream:short)%09%(upstream:track)%09%(committerdate:unix)%09%(objectname)",
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
        let expected_oid = f.next().unwrap_or("").trim().to_string();

        if branch.is_empty()
            || expected_oid.is_empty()
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

        // Work out what actually survives deleting this branch, rather than
        // inferring it from the branch's name or its tracking status.
        let survival = survives(repo, &branch, &merge_base, is_merged);
        let proof = match &survival {
            Survives::Merged => resolved_branch_proof(repo, &merge_base, false),
            Survives::PatchesUpstream { .. } => resolved_branch_proof(repo, &merge_base, true),
            Survives::OnRemote { remote, .. } => resolved_branch_proof(repo, remote, false),
            Survives::LocalOnly { .. } | Survives::Unknown { .. } => Some(BranchProof::None),
        };
        let Verdict {
            group,
            mut detail,
            risk,
            force,
        } = grade(survival, &merge_base, &upstream, is_gone);
        let (action, eligibility) = if let Some(proof) = proof {
            (
                Action::GitBranchDelete {
                    repo: repo.to_path_buf(),
                    branch: branch.clone(),
                    expected_oid,
                    proof,
                    force,
                },
                Eligibility::Reclaimable,
            )
        } else {
            detail.push_str(" · proof ref could not be pinned; inspect manually");
            (Action::None, Eligibility::Protected)
        };
        let cand = Candidate::new(
            Category::Git,
            group,
            format!("{repo_name}/{branch}"),
            detail,
            0,
            risk,
            action,
        )
        .with_age(Some(age))
        .with_eligibility(eligibility);
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
        .filter(|s| !s.is_empty());
        return match remote {
            Some(remote) => Survives::OnRemote {
                commits: ahead,
                remote,
            },
            None => Survives::Unknown {
                reason: "git could not identify the remote ref holding these commits",
            },
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

#[derive(Debug)]
struct WorktreeFiles {
    changed: usize,
    generated_ignored: usize,
    unknown_ignored: usize,
    status: String,
}

/// Inspect everything `git worktree remove --force` would discard. Known build
/// output is counted separately from ignored paths such as `.env` or a local
/// database, which may be the only copy of real data.
fn worktree_files(path: &Path, opts: &ScanOpts) -> Option<WorktreeFiles> {
    let status = git(
        path,
        &[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--ignored=matching",
        ],
    )?;
    let mut files = WorktreeFiles {
        changed: 0,
        generated_ignored: 0,
        unknown_ignored: 0,
        status: status.clone(),
    };
    for line in status.lines().filter(|line| !line.trim().is_empty()) {
        if let Some(relative) = line.strip_prefix("!! ") {
            let relative = relative.trim_end_matches('/').trim_matches('"');
            let ignored = path.join(relative);
            if ignored.is_dir()
                && super::artifacts::recognised_path(&ignored, &opts.rules.artifacts)
            {
                files.generated_ignored += 1;
            } else {
                files.unknown_ignored += 1;
            }
        } else {
            files.changed += 1;
        }
    }
    Some(files)
}

/// Whether removing the checkout leaves its commits reachable. An attached
/// branch is itself the surviving ref; "not pushed" is not "lost" here because
/// `git worktree remove` does not delete that branch.
fn worktree_commits_survive(repo: &Path, wt: &Worktree) -> Option<bool> {
    if wt.branch.is_some() {
        return Some(true);
    }
    let head = git(&wt.path, &["rev-parse", "HEAD"])?;
    let refs = git(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "--contains",
            head.trim(),
        ],
    )?;
    Some(refs.lines().any(|line| !line.trim().is_empty()))
}

fn worktree_age(path: &Path, newest_file: Option<u64>) -> Option<u64> {
    let commit = git(path, &["log", "-1", "--format=%ct", "HEAD"])
        .and_then(|value| value.trim().parse::<u64>().ok());
    newest_file.max(commit).map(days_since)
}

fn worktree_verdict(
    branch: &str,
    files: Option<&WorktreeFiles>,
    commits_survive: Option<bool>,
) -> (Risk, String) {
    let (risk, detail) = match (files, commits_survive) {
        (None, _) => (
            Risk::Danger,
            "git could not inspect files in this worktree".to_string(),
        ),
        (_, None) => (
            Risk::Danger,
            "git could not verify where its commits survive".to_string(),
        ),
        (Some(files), survives) if files.changed > 0 || files.unknown_ignored > 0 => {
            let mut losses = Vec::new();
            if files.changed > 0 {
                losses.push(format!("{} uncommitted file(s)", files.changed));
            }
            if files.unknown_ignored > 0 {
                losses.push(format!("{} unknown ignored", files.unknown_ignored));
            }
            if survives == Some(false) {
                losses.push("detached HEAD has no surviving ref".to_string());
            }
            (Risk::Danger, losses.join(" · "))
        }
        (Some(files), Some(false)) => (
            Risk::Danger,
            format!(
                "detached HEAD has no surviving ref · {} generated ignored paths",
                files.generated_ignored
            ),
        ),
        (Some(files), Some(true)) if files.generated_ignored > 0 => (
            Risk::Caution,
            format!(
                "commits remain reachable · {} generated ignored paths are rebuilt",
                files.generated_ignored
            ),
        ),
        (Some(_), Some(true)) => (
            Risk::Caution,
            "clean · commits remain reachable · safe to prune and recreate".to_string(),
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
        if wt.bare {
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

        let (size, newest_file) = opts.cache.measure(&wt.path);
        let age = worktree_age(&wt.path, newest_file).unwrap_or(0);
        if wt.locked {
            let candidate = Candidate::new(
                Category::Git,
                "protected worktrees",
                label,
                format!("[{branch}] explicitly locked with git worktree lock"),
                size,
                Risk::Danger,
                Action::None,
            )
            .with_age(Some(age))
            .with_footprint(wt.path.clone())
            .with_eligibility(Eligibility::Protected);
            super::emit(tx, opts, candidate);
            continue;
        }

        let files = worktree_files(&wt.path, opts);
        let commits_survive = worktree_commits_survive(repo, wt);
        let (risk, detail) = worktree_verdict(&branch, files.as_ref(), commits_survive);
        let head = git(&wt.path, &["rev-parse", "HEAD"]).map(|head| head.trim().to_string());

        let (action, eligibility) = match (head, files.as_ref()) {
            (Some(expected_head), Some(files)) => (
                Action::GitWorktreeRemove {
                    repo: repo.to_path_buf(),
                    path: wt.path.clone(),
                    expected_head,
                    expected_status: files.status.clone(),
                    require_surviving_ref: wt.branch.is_none() && commits_survive == Some(true),
                    force: risk == Risk::Danger,
                },
                Eligibility::Reclaimable,
            ),
            _ => (Action::None, Eligibility::Protected),
        };

        let candidate = Candidate::new(
            Category::Git,
            "stale worktrees",
            label,
            detail,
            size,
            risk,
            action,
        )
        .with_age(Some(age))
        .with_footprint(wt.path.clone())
        .with_eligibility(eligibility);
        super::emit(tx, opts, candidate);
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
        // Stash selectors are mutable reflog positions. Git has no atomic
        // compare-and-drop operation for an arbitrary entry, so reap keeps the
        // evidence visible but refuses to automate a potentially wrong delete.
        let cand = Candidate::new(
            Category::Git,
            "protected stashes",
            format!("{repo_name}: {reflog}"),
            format!("{subject} · mutable stash position; inspect and drop manually"),
            0,
            Risk::Danger,
            Action::None,
        )
        .with_age(Some(age))
        .with_eligibility(Eligibility::Protected);
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
    let candidate = Candidate::new(
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
    let candidate = match git_objects_dir(repo) {
        Some(path) => candidate.with_footprint(path),
        None => candidate,
    };
    super::emit(tx, opts, candidate);
}
