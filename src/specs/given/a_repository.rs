//! A real git repository with a real remote, built one branch at a time.

use super::{git, scanning_everything, scratch};
use crate::model::{Candidate, ScanEvent};
use std::path::PathBuf;

/// A working repository whose `origin` is a bare repository beside it.
///
/// A remote is not optional scenery here: whether a branch is recoverable turns
/// entirely on what a remote can still reach, so it has to be genuine.
pub struct a_repository {
    dir: scratch,
    work: PathBuf,
}

impl a_repository {
    pub fn new() -> Self {
        let dir = scratch::named("repo");
        let work = dir.path.join("work");
        let origin = dir.path.join("origin.git");
        std::fs::create_dir_all(&work).unwrap();
        std::fs::create_dir_all(&origin).unwrap();

        git(&origin, &["init", "--bare", "-b", "main"]);
        git(&work, &["init", "-b", "main"]);
        git(
            &work,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );

        let this = Self { dir, work };
        this.commit("README.md", "initial");
        git(&this.work, &["push", "-u", "origin", "main"]);
        this
    }

    fn commit(&self, file: &str, message: &str) {
        std::fs::write(self.work.join(file), format!("{message}\n")).unwrap();
        git(&self.work, &["add", file]);
        git(&self.work, &["commit", "-m", message]);
    }

    fn branch_with_a_commit(&self, name: &str) {
        git(&self.work, &["checkout", "-q", "-b", name]);
        // Branch names contain slashes; file names in the root must not.
        self.commit(
            &format!("{}.txt", name.replace('/', "-")),
            &format!("work on {name}"),
        );
        git(&self.work, &["checkout", "-q", "main"]);
    }

    /// Merged the ordinary way, so the branch is an ancestor of main.
    pub fn with_a_branch_merged_into_main(self, name: &str) -> Self {
        self.branch_with_a_commit(name);
        git(&self.work, &["merge", "--no-ff", "-m", "merge", name]);
        git(&self.work, &["push", "origin", "main"]);
        self
    }

    /// Squash-merged: the work is in main by content, but the branch is not an
    /// ancestor of it and `--merged` will not report it.
    pub fn with_a_branch_squash_merged_into_main(self, name: &str) -> Self {
        self.branch_with_a_commit(name);
        git(&self.work, &["merge", "--squash", name]);
        git(&self.work, &["commit", "-m", &format!("squash {name}")]);
        git(&self.work, &["push", "origin", "main"]);
        self
    }

    /// Both ordinary patches are upstream, but a merge commit itself introduced
    /// content that exists only on the branch. `git cherry` omits that merge.
    pub fn with_a_branch_holding_unique_merge_content(self, name: &str) -> Self {
        git(&self.work, &["checkout", "-q", "-b", name]);
        self.commit("feature.txt", "feature patch");
        let feature = git(&self.work, &["rev-parse", "HEAD"]);

        git(
            &self.work,
            &["checkout", "-q", "-b", "fixture-side", "main"],
        );
        self.commit("side.txt", "side patch");
        let side = git(&self.work, &["rev-parse", "HEAD"]);

        git(&self.work, &["checkout", "-q", name]);
        git(
            &self.work,
            &["merge", "--no-ff", "-m", "merge side", "fixture-side"],
        );
        std::fs::write(self.work.join("merge-only.txt"), "unique resolution\n").unwrap();
        git(&self.work, &["add", "merge-only.txt"]);
        git(&self.work, &["commit", "--amend", "--no-edit"]);

        git(&self.work, &["checkout", "-q", "main"]);
        git(&self.work, &["cherry-pick", &feature]);
        git(&self.work, &["cherry-pick", &side]);
        git(&self.work, &["push", "origin", "main"]);
        self
    }

    /// Unmerged, but every commit is on the remote and can be fetched back.
    pub fn with_a_branch_pushed_but_unmerged(self, name: &str) -> Self {
        self.branch_with_a_commit(name);
        git(&self.work, &["push", "-u", "origin", name]);
        self
    }

    /// Commits that exist in this clone and nowhere else.
    pub fn with_a_branch_never_pushed(self, name: &str) -> Self {
        self.branch_with_a_commit(name);
        self
    }

    /// Pushed, then deleted on the remote — what a merged pull request leaves
    /// behind, and also what losing work looks like.
    pub fn with_a_branch_whose_upstream_was_deleted(self, name: &str) -> Self {
        self.branch_with_a_commit(name);
        git(&self.work, &["push", "-u", "origin", name]);
        git(&self.work, &["push", "origin", "--delete", name]);
        git(&self.work, &["fetch", "--prune"]);
        self
    }

    /// A linked worktree checked out on its own branch.
    pub fn with_a_worktree(self, name: &str) -> Self {
        let path = self.dir.path.join(name);
        git(
            &self.work,
            &["worktree", "add", "-b", name, path.to_str().unwrap()],
        );
        self
    }

    /// A worktree holding a commit no remote can reach. The attached branch
    /// still keeps it reachable after the checkout is removed.
    pub fn with_a_worktree_holding_unpushed_work(self, name: &str) -> Self {
        let this = self.with_a_worktree(name);
        let path = this.dir.path.join(name);
        std::fs::write(path.join("wip.txt"), "wip\n").unwrap();
        git(&path, &["add", "wip.txt"]);
        git(&path, &["commit", "-m", "wip"]);
        this
    }

    /// A detached worktree whose HEAD is not named by any surviving ref.
    pub fn with_a_detached_worktree_holding_unique_commit(self, name: &str) -> Self {
        let path = self.dir.path.join(name);
        git(
            &self.work,
            &[
                "worktree",
                "add",
                "--detach",
                path.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(path.join("detached.txt"), "detached work\n").unwrap();
        git(&path, &["add", "detached.txt"]);
        git(&path, &["commit", "-m", "detached work"]);
        self
    }

    /// A checkout-shaped agent directory whose Git registration is gone.
    pub fn with_an_orphaned_agent_worktree(self, name: &str) -> Self {
        let path = self.work.join(".claude/worktrees").join(name);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join(".git"), "gitdir: /missing/worktree/admin\n").unwrap();
        std::fs::write(path.join("only-copy.txt"), "unknown work\n").unwrap();
        self
    }

    /// A worktree with changes that were never committed at all.
    pub fn with_a_dirty_worktree(self, name: &str) -> Self {
        let this = self.with_a_worktree(name);
        std::fs::write(this.dir.path.join(name).join("scratch.txt"), "unsaved\n").unwrap();
        this
    }

    pub fn with_a_stash(self, message: &str) -> Self {
        std::fs::write(self.work.join("stashed.txt"), "in progress\n").unwrap();
        git(&self.work, &["add", "stashed.txt"]);
        git(&self.work, &["stash", "push", "-m", message]);
        self
    }

    /// A worktree that looks clean unless Git is explicitly asked for ignored
    /// files. Forced removal would delete the file along with the checkout.
    pub fn with_a_worktree_holding_an_ignored_file(self, name: &str) -> Self {
        std::fs::write(self.work.join(".gitignore"), "ignored-only.bin\n").unwrap();
        git(&self.work, &["add", ".gitignore"]);
        git(&self.work, &["commit", "-m", "ignore generated files"]);
        git(&self.work, &["push", "origin", "main"]);

        let this = self.with_a_worktree(name);
        std::fs::write(
            this.dir.path.join(name).join("ignored-only.bin"),
            "the only copy\n",
        )
        .unwrap();
        this
    }

    /// Everything the git scanner reports for this repository.
    ///
    /// The act under specification: run the real scanner and collect what it
    /// produced, rather than reaching into its internals.
    pub fn candidates(self) -> Vec<Candidate> {
        let opts = scanning_everything(vec![self.dir.path.clone()]);
        let (tx, rx) = std::sync::mpsc::channel();
        crate::scan::git::scan(std::slice::from_ref(&self.work), &opts, &tx);
        drop(tx);

        rx.into_iter()
            .filter_map(|e| match e {
                ScanEvent::Found(c) => Some(*c),
                _ => None,
            })
            .collect()
    }
}
