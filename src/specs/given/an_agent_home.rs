//! A coding agent's session store, and the projects it kept them for.
//!
//! Two things here are built for real rather than described, because both are
//! things the scanner has to work out for itself. The projects are real
//! directories at real paths, since the behaviour under specification is
//! whether reap can find its way from a flattened directory name back to a
//! checkout that may or may not still exist.
//!
//! And every timestamp is set deliberately, *deepest first*, because the
//! question "is this session finished" is answered from the newest file
//! anywhere inside a store and not from the store's own timestamp. A fixture
//! that only aged the directories would agree with a scanner that only read
//! them, and the two would be wrong together.

use super::{back_date, back_date_tree, scratch};
use crate::config::{AgentRule, Config, RiskName};
use crate::model::{Candidate, ScanEvent};
use std::path::{Path, PathBuf};

/// The tool a configured rule is filed under, used to tell the fixture's own
/// rows apart from any the machine running the specs contributes.
const CONFIGURED_TOOL: &str = "a configured agent";

/// How long ago a store that is finished was last written to.
///
/// Far past every threshold in play, so a specification that says nothing about
/// age is not quietly also a specification about age.
const LONG_FINISHED_DAYS: u64 = 120;

pub struct an_agent_home {
    dir: scratch,
    tool: &'static str,
    /// Transcripts to stamp after the tree is aged: `(file, seconds ago)`.
    ///
    /// Applied last, so they survive the back-dating of everything around them
    /// — which is exactly the shape the real case has, a fresh file inside
    /// directories nothing has added to for months.
    still_warm: Vec<(PathBuf, u64)>,
}

impl an_agent_home {
    pub fn new() -> Self {
        Self {
            dir: scratch::named("agent-home"),
            tool: "an agent",
            still_warm: Vec::new(),
        }
    }

    /// Where the projects themselves live.
    fn projects(&self) -> PathBuf {
        self.dir.path.join("checkouts")
    }

    /// Where the tool keeps what it remembers about them.
    fn store(&self) -> PathBuf {
        self.dir.path.join("store")
    }

    /// A project that still exists, and the finished sessions held for it.
    pub fn with_sessions_for_a_project_named(self, name: &str, bytes: usize) -> Self {
        let project = self.projects().join(name);
        std::fs::create_dir_all(&project).unwrap();
        self.holding(&flatten(&project), bytes)
    }

    /// A project whose agent is talking to it right now.
    ///
    /// The transcript is being appended to; nothing is being added to the
    /// directory holding it, which is why the directory's own timestamp is the
    /// wrong thing to read.
    pub fn with_a_session_still_being_written_for(self, name: &str, bytes: usize) -> Self {
        self.with_a_session_last_written_for(name, 0, bytes)
    }

    /// A project whose last session ended `hours_ago`.
    pub fn with_a_session_last_written_for(
        mut self,
        name: &str,
        hours_ago: u64,
        bytes: usize,
    ) -> Self {
        let project = self.projects().join(name);
        std::fs::create_dir_all(&project).unwrap();
        let store = self.store().join(flatten(&project));
        self.still_warm
            .push((store.join(TRANSCRIPT), hours_ago * 3600));
        self.holding(&flatten(&project), bytes)
    }

    /// Sessions held for a project that is no longer on disk.
    pub fn with_sessions_for_a_project_since_deleted(self, name: &str, bytes: usize) -> Self {
        let gone = self.projects().join(name);
        self.holding(&flatten(&gone), bytes)
    }

    /// Sessions filed under a name that is not a path at all.
    pub fn with_sessions_filed_under(self, opaque: &str, bytes: usize) -> Self {
        self.holding(opaque, bytes)
    }

    /// A day of sessions, for a tool that files them by date.
    pub fn with_a_days_sessions_on(self, date: &str, bytes: usize) -> Self {
        self.holding(date, bytes)
    }

    /// A day of sessions where one is still open.
    ///
    /// Two directories down from the month that will be offered, so a fixture
    /// using this is asking whether the freshness of a file is noticed from the
    /// row that would delete it.
    pub fn with_a_days_sessions_still_being_written_on(mut self, date: &str, bytes: usize) -> Self {
        self.still_warm
            .push((self.store().join(date).join(TRANSCRIPT), 0));
        self.holding(date, bytes)
    }

    /// A session written straight into the store, with no directory of its own.
    ///
    /// The shape a tool that reap has read wrong presents: nothing to walk, and
    /// bytes that a scanner looking only for subdirectories would never see.
    pub fn with_sessions_in_a_loose_file(self, name: &str, bytes: usize) -> Self {
        let store = self.store();
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(store.join(name), vec![0u8; bytes]).unwrap();
        self
    }

    fn holding(self, name: &str, bytes: usize) -> Self {
        let path = self.store().join(name);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join(TRANSCRIPT), vec![0u8; bytes]).unwrap();
        self
    }

    /// Age everything, then put the warm transcripts back where they were.
    ///
    /// Order matters and is the whole point: writing a file updates the
    /// directory holding it, so a tree aged before its last write would be
    /// aged and then immediately undone.
    fn settle(&self) {
        back_date_tree(&self.store(), LONG_FINISHED_DAYS * 86_400);
        for (file, seconds_ago) in &self.still_warm {
            back_date(file, *seconds_ago);
        }
    }

    /// What the scanner reports for a store laid out one directory per project.
    pub fn candidates(self) -> Vec<Candidate> {
        self.settle();
        let opts = super::scanning_everything(vec![]);
        let (tx, rx) = std::sync::mpsc::channel();
        crate::scan::agents::per_project(self.tool, &self.store(), &opts, &tx);
        drop(tx);
        collect(rx)
    }

    /// What the whole scan reports for a directory named by a configured rule.
    ///
    /// Goes through `scan` rather than through one of the two functions above,
    /// because what is under specification is the wiring: that a rule someone
    /// wrote in a configuration file reaches the screen at all, in the right
    /// category and at the risk it asked for. The built-in rules are replaced
    /// rather than added to, so this observes the fixture rather than the
    /// machine the specs happen to be running on.
    pub fn candidates_for_a_configured_rule(self, risk: RiskName) -> Vec<Candidate> {
        self.settle();
        let cfg = Config {
            replace_builtin_agents: true,
            agents: vec![AgentRule {
                path: self.store().to_string_lossy().into_owned(),
                tool: CONFIGURED_TOOL.to_string(),
                label: "a configured directory".to_string(),
                detail: "put there by a rule".to_string(),
                risk,
            }],
            ..Config::default()
        };
        let opts = crate::scan::ScanOpts {
            rules: std::sync::Arc::new(crate::scan::Rules::from_config(&cfg)),
            ..super::scanning_everything(vec![])
        };

        let (tx, rx) = std::sync::mpsc::channel();
        crate::scan::agents::scan(&opts, &tx);
        drop(tx);
        // The session-store layouts are not configurable, so a machine with
        // real agent data on it contributes rows here too. Those are somebody
        // else's tool by definition.
        collect(rx)
            .into_iter()
            .filter(|c| c.group == CONFIGURED_TOOL)
            .collect()
    }

    /// What the scanner reports for a store laid out `YYYY/MM/DD`.
    pub fn candidates_by_month(self) -> Vec<Candidate> {
        self.settle();
        let opts = super::scanning_everything(vec![]);
        let (tx, rx) = std::sync::mpsc::channel();
        crate::scan::agents::by_month(self.tool, &self.store(), &opts, &tx);
        drop(tx);
        collect(rx)
    }
}

/// What a session is written into. Appended to for as long as it runs.
const TRANSCRIPT: &str = "transcript.jsonl";

/// A path written the way the agents write it: separators replaced by dashes.
fn flatten(path: &Path) -> String {
    let mut flat = String::new();
    for part in path.components().skip(1) {
        flat.push('-');
        flat.push_str(&part.as_os_str().to_string_lossy());
    }
    flat
}

fn collect(rx: std::sync::mpsc::Receiver<ScanEvent>) -> Vec<Candidate> {
    rx.into_iter()
        .filter_map(|e| match e {
            ScanEvent::Found(c) => Some(*c),
            _ => None,
        })
        .collect()
}
