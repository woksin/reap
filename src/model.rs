use std::path::PathBuf;

/// Top-level buckets shown in the sidebar.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Category {
    /// Read-only catalogue rows that explain occupied space but are not deletion
    /// candidates. Keeping these beside, rather than inside, the reclaim
    /// categories lets reap answer "where did the disk go?" without implying
    /// that ordinary data is debris.
    Storage,
    Git,
    Artifacts,
    Docker,
    Caches,
    /// What the coding agents leave behind: their caches and package trees,
    /// and the conversation history they keep per project. The first half is
    /// ordinary debris; the second is graded like `Personal`, because nothing
    /// regenerates a transcript.
    Agents,
    /// Things that belong to the person rather than to the machine: downloads,
    /// installers, device backups. Graded far more carefully than the rest,
    /// because nothing here regenerates itself.
    Personal,
}

impl Category {
    pub const ALL: [Self; 7] = [
        Self::Storage,
        Self::Git,
        Self::Artifacts,
        Self::Docker,
        Self::Caches,
        Self::Agents,
        Self::Personal,
    ];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Storage => "Disk usage",
            Self::Git => "Git",
            Self::Artifacts => "Build artifacts",
            Self::Docker => "Docker",
            Self::Caches => "Caches",
            Self::Agents => "Coding agents",
            Self::Personal => "Personal",
        }
    }
}

/// How dangerous it is to delete a candidate.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Eligibility {
    /// Old enough, sufficiently understood, and available for selection.
    Reclaimable,
    /// Recognised content that has not reached the configured age.
    Recent,
    /// Evidence says a process or tool is still using it.
    Active,
    /// Explicitly retained, locked, or otherwise not safe to offer.
    Protected,
    /// Ordinary or unknown disk usage shown only to explain occupied space.
    Informational,
}

impl Eligibility {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Reclaimable => "reclaimable",
            Self::Recent => "recent",
            Self::Active => "active",
            Self::Protected => "protected",
            Self::Informational => "usage",
        }
    }
}

/// How dangerous it is to delete a candidate.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Risk {
    /// Regenerated automatically, no human input lost.
    Safe,
    /// Costs time to rebuild or re-download, but nothing is unrecoverable.
    Caution,
    /// May destroy work that exists nowhere else.
    Danger,
}

impl Risk {
    pub const fn dot(self) -> &'static str {
        match self {
            // The two recoverable levels share a glyph and are told apart by
            // colour; only the irreversible one changes shape, so it still
            // reads as a warning where colour is missing.
            Self::Safe | Self::Caution => "●",
            Self::Danger => "▲",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Caution => "rebuildable",
            Self::Danger => "irreversible",
        }
    }
}

#[derive(Clone, Debug)]
pub enum BranchProof {
    /// The scanned OID must remain reachable from this exact ref, whose tip is
    /// atomically verified alongside branch deletion.
    ContainsRef {
        reference: String,
        expected_tip: String,
    },
    /// Every linear patch must still be present in this exact upstream tip,
    /// which is atomically verified alongside branch deletion.
    PatchesIn {
        reference: String,
        expected_tip: String,
    },
    /// Irreversible deletion was explicitly confirmed; no recoverability claim.
    None,
}

/// What actually happens when a candidate is reaped.
#[derive(Clone, Debug)]
pub enum Action {
    /// Inventory and protected rows deliberately have no destructive action.
    None,
    /// Recursively remove a path.
    Remove(PathBuf),
    /// Atomically delete exactly the branch OID that was assessed, after
    /// rechecking the recoverability proof used for its risk grade.
    GitBranchDelete {
        repo: PathBuf,
        branch: String,
        expected_oid: String,
        proof: BranchProof,
        force: bool,
    },
    /// Remove a linked worktree only if HEAD and status still match the scan.
    /// This closes the race where an agent resumes work after the row appears.
    GitWorktreeRemove {
        repo: PathBuf,
        path: PathBuf,
        expected_head: String,
        expected_status: String,
        /// Detached HEAD was considered rebuildable only because another ref
        /// contained it; that proof must still hold at execution time.
        require_surviving_ref: bool,
        force: bool,
    },
    /// Shell out, e.g. `docker rmi` or a package-manager cleaner.
    Run {
        program: String,
        args: Vec<String>,
        cwd: Option<PathBuf>,
    },
}

impl Action {
    pub fn describe(&self) -> String {
        match self {
            Self::None => "not selectable".to_string(),
            Self::Remove(p) => format!("rm -rf {}", crate::util::tilde(p)),
            Self::GitBranchDelete {
                repo,
                branch,
                force,
                ..
            } => format!(
                "git branch {} {}  # atomic OID check in {}",
                if *force { "-D" } else { "-d" },
                branch,
                crate::util::tilde(repo)
            ),
            Self::GitWorktreeRemove {
                repo, path, force, ..
            } => format!(
                "git worktree remove {}{}  # in {}",
                if *force { "--force " } else { "" },
                crate::util::tilde(path),
                crate::util::tilde(repo)
            ),
            Self::Run { program, args, .. } => {
                format!("{program} {}", args.join(" "))
            }
        }
    }
}

/// One recognised disk finding. Eligibility decides whether it is currently
/// reapable or present only to explain occupied bytes.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub category: Category,
    /// Sub-bucket within the category, e.g. "build cache", "merged branches".
    pub group: String,
    /// Primary display name.
    pub label: String,
    /// Secondary context line — where it lives, why it is stale.
    pub detail: String,
    pub size: u64,
    /// `None` when age is not meaningful (e.g. docker build cache aggregate).
    pub age_days: Option<u64>,
    pub risk: Risk,
    /// Whether this row can currently be acted on. Risk describes the cost if
    /// it can; eligibility describes whether it is presently a candidate.
    pub eligibility: Eligibility,
    pub action: Action,
    /// The filesystem tree affected by the action, including command actions
    /// such as `git worktree remove`. Used to avoid counting or executing a
    /// nested artifact twice.
    pub footprint: Option<PathBuf>,
    /// Programs whose running presence means this must be left alone. Empty for
    /// almost everything; see [`crate::liveness`].
    pub owner: Vec<String>,
    pub selected: bool,
}

impl Candidate {
    pub fn new(
        category: Category,
        group: impl Into<String>,
        label: impl Into<String>,
        detail: impl Into<String>,
        size: u64,
        risk: Risk,
        action: Action,
    ) -> Self {
        let footprint = match &action {
            Action::Remove(path) | Action::GitWorktreeRemove { path, .. } => Some(path.clone()),
            Action::None | Action::GitBranchDelete { .. } | Action::Run { .. } => None,
        };
        // A row with nothing to run cannot give any disk back, so it must not
        // default to the one eligibility that says it can. Every scanner that
        // builds an actionless row already states an eligibility and overrides
        // this, which is exactly why the wrong default was invisible: it waits
        // for the first row that forgets, and then quietly promises bytes no
        // keystroke can deliver.
        let eligibility = match action {
            Action::None => Eligibility::Protected,
            _ => Eligibility::Reclaimable,
        };
        Self {
            category,
            group: group.into(),
            label: label.into(),
            detail: detail.into(),
            size,
            age_days: None,
            risk,
            eligibility,
            action,
            footprint,
            owner: Vec::new(),
            selected: false,
        }
    }

    pub const fn with_age(mut self, days: Option<u64>) -> Self {
        self.age_days = days;
        self
    }

    pub fn with_owner(mut self, owner: Vec<String>) -> Self {
        self.owner = owner;
        self
    }

    pub fn with_footprint(mut self, path: PathBuf) -> Self {
        self.footprint = Some(path);
        self
    }

    pub const fn with_eligibility(mut self, eligibility: Eligibility) -> Self {
        self.eligibility = eligibility;
        self
    }

    pub const fn selectable(&self) -> bool {
        matches!(self.eligibility, Eligibility::Reclaimable) && !matches!(self.action, Action::None)
    }
}

/// Messages from the scan threads to the UI.
pub enum ScanEvent {
    Found(Box<Candidate>),
    Status(String),
    /// A single scanner finished.
    Done(Category),
}

/// Messages from the reaper thread to the UI.
pub enum ReapEvent {
    Progress(Box<crate::reaper::Report>),
    Finished,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn candidate(action: Action) -> Candidate {
        Candidate::new(Category::Git, "g", "l", "", 100, Risk::Safe, action)
    }

    /// The header promises a reclaimable total and splits it by risk. A row
    /// with nothing to run returns no disk, so defaulting it to `Reclaimable`
    /// would put bytes into that promise that no keystroke can deliver.
    ///
    /// Every scanner that builds an actionless row states an eligibility and
    /// overrides the default, which is precisely why the wrong one was
    /// invisible: it waits for the first caller that forgets.
    #[test]
    fn a_row_with_nothing_to_run_is_not_offered_as_reclaimable() {
        let cand = candidate(Action::None);
        assert_eq!(cand.eligibility, Eligibility::Protected);
        assert!(!cand.selectable());
    }

    #[test]
    fn a_row_that_removes_a_path_is_reclaimable_by_default() {
        let cand = candidate(Action::Remove(PathBuf::from("/work/project/target")));
        assert_eq!(cand.eligibility, Eligibility::Reclaimable);
        assert!(cand.selectable());
    }

    /// An explicit eligibility still wins, or the scanners could not mark a
    /// live Docker image active or a locked worktree protected.
    #[test]
    fn an_explicit_eligibility_overrides_the_default() {
        let cand = candidate(Action::Remove(PathBuf::from("/work/project/target")))
            .with_eligibility(Eligibility::Recent);
        assert_eq!(cand.eligibility, Eligibility::Recent);
        assert!(!cand.selectable());
    }
}
