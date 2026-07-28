use std::path::PathBuf;

/// Top-level buckets shown in the sidebar.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Category {
    Git,
    Artifacts,
    Docker,
    Caches,
    /// Things that belong to the person rather than to the machine: downloads,
    /// installers, device backups. Graded far more carefully than the rest,
    /// because nothing here regenerates itself.
    Personal,
}

impl Category {
    pub const ALL: [Self; 5] = [
        Self::Git,
        Self::Artifacts,
        Self::Docker,
        Self::Caches,
        Self::Personal,
    ];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Git => "Git",
            Self::Artifacts => "Build artifacts",
            Self::Docker => "Docker",
            Self::Caches => "Caches",
            Self::Personal => "Personal",
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

/// What actually happens when a candidate is reaped.
#[derive(Clone, Debug)]
pub enum Action {
    /// Recursively remove a path.
    Remove(PathBuf),
    /// Shell out, e.g. `git worktree remove` or `docker rmi`.
    Run {
        program: String,
        args: Vec<String>,
        cwd: Option<PathBuf>,
    },
}

impl Action {
    pub fn describe(&self) -> String {
        match self {
            Self::Remove(p) => format!("rm -rf {}", crate::util::tilde(p)),
            Self::Run { program, args, .. } => {
                format!("{program} {}", args.join(" "))
            }
        }
    }
}

/// One reapable thing.
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
    pub action: Action,
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
        Self {
            category,
            group: group.into(),
            label: label.into(),
            detail: detail.into(),
            size,
            age_days: None,
            risk,
            action,
            selected: false,
        }
    }

    pub const fn with_age(mut self, days: Option<u64>) -> Self {
        self.age_days = days;
        self
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
