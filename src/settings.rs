//! The settings screen: everything reap is working from, and the means to
//! change it without leaving.
//!
//! reap's behaviour has always been entirely configurable and almost entirely
//! invisible. Ninety cache rules, thirty artifact rules, a dozen recipes and
//! five thresholds decide what you are shown, and the only way to read any of
//! it was to open the source or to guess from what appeared. Two consequences
//! followed. Someone whose setup did not match the built-in assumptions had no
//! way to find out *which* assumption was wrong. And `x` — never offer this
//! again — was a one-way door: it wrote a line to a file nobody was looking at,
//! and there was no screen on which to take it back.
//!
//! So this is a list of every rule, where it came from, and whether it is on.
//! Editing writes to the same `config.toml` a person would have edited by hand,
//! in the same shapes, so nothing learned here stops being true at the command
//! line — and a file hand-written first is read, shown, and edited in place
//! rather than being replaced by whatever the screen thought it knew.
//!
//! What it deliberately does not do is invent a second way to say things. Every
//! action here writes an `ignore`, an `[[override]]`, a `[[cache]]`, an
//! `[[artifact]]`, a root or a threshold — the vocabulary the config file and
//! the README already use.

use crate::config::{ArtifactRule, CacheRule, Config, OverrideRule, RecipeRule, RiskName};
use std::collections::HashSet;

/// Where a rule came from, which decides what may be done to it.
///
/// A built-in can be turned off and re-graded but never edited or deleted:
/// changing one in place would mean the config no longer describes a change to
/// reap's defaults but a replacement for them, and the next release moving a
/// vendor's cache directory would silently stop applying.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    Builtin,
    Yours,
}

impl Origin {
    pub fn label(self) -> &'static str {
        match self {
            Origin::Builtin => "built-in",
            Origin::Yours => "yours",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Section {
    Roots,
    Scanning,
    Caches,
    Artifacts,
    Ignores,
    Overrides,
    Recipes,
}

impl Section {
    pub const ALL: [Section; 7] = [
        Section::Roots,
        Section::Scanning,
        Section::Caches,
        Section::Artifacts,
        Section::Ignores,
        Section::Overrides,
        Section::Recipes,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Section::Roots => "Where to look",
            Section::Scanning => "Scanning",
            Section::Caches => "Caches",
            Section::Artifacts => "Build artifacts",
            Section::Ignores => "Never offer",
            Section::Overrides => "Re-graded",
            Section::Recipes => "Quick reaps",
        }
    }

    /// One line on what the section is for, shown under the heading.
    pub fn blurb(self) -> &'static str {
        match self {
            Section::Roots => "directories searched for repositories and build output",
            Section::Scanning => "thresholds, and which scanners run at all",
            Section::Caches => "a path, what clears it, and what it costs to lose",
            Section::Artifacts => "directory names, and the sibling file that proves what they are",
            Section::Ignores => "patterns reap will never put in front of you",
            Section::Overrides => "risk gradings you disagreed with",
            Section::Recipes => "one key per standing decision · edit these in the config file",
        }
    }

    /// What `a` adds here, when the section takes additions at all.
    pub fn adds(self) -> Option<&'static str> {
        match self {
            Section::Roots => Some("a directory to search"),
            Section::Caches => Some("a cache path"),
            Section::Artifacts => Some("a build directory name"),
            Section::Ignores => Some("a pattern never to offer"),
            Section::Scanning | Section::Overrides | Section::Recipes => None,
        }
    }
}

/// A single value under `[scan]`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Setting {
    StaleDays,
    MinSize,
    Depth,
    LibraryCacheFloor,
    DownloadsFloor,
    Docker,
    Caches,
    Personal,
    Trash,
}

impl Setting {
    pub const ALL: [Setting; 9] = [
        Setting::StaleDays,
        Setting::MinSize,
        Setting::Depth,
        Setting::LibraryCacheFloor,
        Setting::DownloadsFloor,
        Setting::Docker,
        Setting::Caches,
        Setting::Personal,
        Setting::Trash,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Setting::StaleDays => "stale after",
            Setting::MinSize => "hide anything under",
            Setting::Depth => "descend at most",
            Setting::LibraryCacheFloor => "unnamed caches over",
            Setting::DownloadsFloor => "downloads over",
            Setting::Docker => "scan Docker",
            Setting::Caches => "scan caches",
            Setting::Personal => "scan your own files",
            Setting::Trash => "trash instead of deleting",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Setting::StaleDays => "days untouched before something counts as stale",
            Setting::MinSize => "the floor under everything reap reports",
            Setting::Depth => "levels below each root",
            Setting::LibraryCacheFloor => "the floor for caches no rule names",
            Setting::DownloadsFloor => "the floor for entries in your download directory",
            Setting::Docker => "images, containers, volumes, build cache",
            Setting::Caches => "every [[cache]] rule, and the sweeps around them",
            Setting::Personal => "downloads, installers, device backups",
            Setting::Trash => "recoverable, but frees nothing until the trash is emptied",
        }
    }

    pub fn is_switch(self) -> bool {
        matches!(
            self,
            Setting::Docker | Setting::Caches | Setting::Personal | Setting::Trash
        )
    }

    /// The value in force, and whether it was chosen or merely defaulted to.
    pub fn value(self, cfg: &Config) -> (String, Origin) {
        let scan = &cfg.scan;
        let number = |set: Option<String>, default: &str| match set {
            Some(v) => (v, Origin::Yours),
            None => (default.to_string(), Origin::Builtin),
        };
        match self {
            Setting::StaleDays => number(scan.stale_days.map(|v| format!("{v} days")), "30 days"),
            Setting::MinSize => number(scan.min_size.clone(), "1MB"),
            Setting::Depth => number(scan.depth.map(|v| v.to_string()), "8"),
            Setting::LibraryCacheFloor => number(scan.library_cache_floor.clone(), "200MB"),
            Setting::DownloadsFloor => number(scan.downloads_floor.clone(), "100MB"),
            Setting::Docker => switch(scan.docker, true),
            Setting::Caches => switch(scan.caches, true),
            Setting::Personal => switch(scan.personal, true),
            Setting::Trash => switch(scan.trash, false),
        }
    }

    /// True when a switch is on, whether by choice or by default.
    pub fn is_on(self, cfg: &Config) -> bool {
        match self {
            Setting::Docker => cfg.scan.docker.unwrap_or(true),
            Setting::Caches => cfg.scan.caches.unwrap_or(true),
            Setting::Personal => cfg.scan.personal.unwrap_or(true),
            Setting::Trash => cfg.scan.trash.unwrap_or(false),
            _ => true,
        }
    }
}

fn switch(set: Option<bool>, default: bool) -> (String, Origin) {
    match set {
        Some(v) => (on_off(v), Origin::Yours),
        None => (on_off(default), Origin::Builtin),
    }
}

fn on_off(v: bool) -> String {
    if v { "on" } else { "off" }.to_string()
}

/// One line on the screen.
#[derive(Clone, PartialEq, Debug)]
pub enum Row {
    Heading(Section),
    Root(usize),
    Setting(Setting),
    Cache(Origin, usize),
    Artifact(Origin, usize),
    Ignore(usize),
    Override(usize),
    Recipe(Origin, usize),
    /// The `+ add …` affordance at the end of a section that takes them.
    Add(Section),
}

impl Row {
    pub fn section(&self) -> Section {
        match self {
            Row::Heading(s) | Row::Add(s) => *s,
            Row::Root(_) => Section::Roots,
            Row::Setting(_) => Section::Scanning,
            Row::Cache(..) => Section::Caches,
            Row::Artifact(..) => Section::Artifacts,
            Row::Ignore(_) => Section::Ignores,
            Row::Override(_) => Section::Overrides,
            Row::Recipe(..) => Section::Recipes,
        }
    }
}

/// Text being typed, and what it will become when it is accepted.
pub struct Edit {
    pub target: Target,
    pub prompt: String,
    pub buffer: String,
}

/// What a completed edit writes to.
#[derive(Clone, PartialEq, Debug)]
pub enum Target {
    /// `None` while adding, `Some(i)` while changing one that exists.
    Root(Option<usize>),
    Ignore(Option<usize>),
    CachePath(Option<usize>),
    CacheLabel(usize),
    ArtifactDir(Option<usize>),
    Setting(Setting),
}

pub struct Settings {
    builtin_caches: Vec<CacheRule>,
    builtin_artifacts: Vec<ArtifactRule>,
    builtin_recipes: Vec<RecipeRule>,

    pub expanded: HashSet<Section>,
    pub rows: Vec<Row>,
    pub cursor: usize,
    pub edit: Option<Edit>,
    pub status: String,
    /// Set once anything has been written, so leaving can offer a rescan.
    pub changed: bool,
}

impl Settings {
    pub fn new(cfg: &Config) -> Self {
        let mut settings = Self {
            builtin_caches: crate::scan::cache_rules::builtin_rules(),
            builtin_artifacts: crate::scan::artifacts::builtin_rules(),
            builtin_recipes: crate::recipes::builtin(),
            // Opening on the sections a person came here to change. The rule
            // lists are hundreds of lines and would bury everything else.
            expanded: [Section::Roots, Section::Scanning].into_iter().collect(),
            rows: Vec::new(),
            cursor: 0,
            edit: None,
            status: String::new(),
            changed: false,
        };
        settings.rebuild(cfg);
        settings
    }

    // ---- the visible list -----------------------------------------------

    /// Recompute the flattened row list, keeping the cursor on the same row.
    pub fn rebuild(&mut self, cfg: &Config) {
        let previous = self.rows.get(self.cursor).cloned();
        let mut rows = Vec::new();

        for section in Section::ALL {
            rows.push(Row::Heading(section));
            if !self.expanded.contains(&section) {
                continue;
            }
            match section {
                Section::Roots => {
                    rows.extend((0..cfg.scan.roots.len()).map(Row::Root));
                }
                Section::Scanning => {
                    rows.extend(Setting::ALL.into_iter().map(Row::Setting));
                }
                Section::Caches => {
                    rows.extend((0..cfg.caches.len()).map(|i| Row::Cache(Origin::Yours, i)));
                    rows.extend(
                        (0..self.builtin_caches.len()).map(|i| Row::Cache(Origin::Builtin, i)),
                    );
                }
                Section::Artifacts => {
                    rows.extend((0..cfg.artifacts.len()).map(|i| Row::Artifact(Origin::Yours, i)));
                    rows.extend(
                        (0..self.builtin_artifacts.len())
                            .map(|i| Row::Artifact(Origin::Builtin, i)),
                    );
                }
                Section::Ignores => {
                    rows.extend((0..cfg.ignore.len()).map(Row::Ignore));
                }
                Section::Overrides => {
                    rows.extend((0..cfg.overrides.len()).map(Row::Override));
                }
                Section::Recipes => {
                    rows.extend((0..cfg.recipes.len()).map(|i| Row::Recipe(Origin::Yours, i)));
                    rows.extend(
                        (0..self.builtin_recipes.len()).map(|i| Row::Recipe(Origin::Builtin, i)),
                    );
                }
            }
            if section.adds().is_some() {
                rows.push(Row::Add(section));
            }
        }

        self.rows = rows;
        if let Some(previous) = previous
            && let Some(i) = self.rows.iter().position(|r| *r == previous)
        {
            self.cursor = i;
        }
        self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
    }

    pub fn current(&self) -> Option<&Row> {
        self.rows.get(self.cursor)
    }

    pub fn move_cursor(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() as isize - 1;
        self.cursor = (self.cursor as isize + delta).clamp(0, last) as usize;
    }

    pub fn toggle_section(&mut self, cfg: &Config) {
        let Some(Row::Heading(section)) = self.current().cloned() else {
            return;
        };
        if !self.expanded.remove(&section) {
            self.expanded.insert(section);
        }
        self.rebuild(cfg);
    }

    // ---- reading a row --------------------------------------------------

    pub fn cache_rule<'a>(
        &'a self,
        cfg: &'a Config,
        origin: Origin,
        i: usize,
    ) -> Option<&'a CacheRule> {
        match origin {
            Origin::Builtin => self.builtin_caches.get(i),
            Origin::Yours => cfg.caches.get(i),
        }
    }

    pub fn artifact_rule<'a>(
        &'a self,
        cfg: &'a Config,
        origin: Origin,
        i: usize,
    ) -> Option<&'a ArtifactRule> {
        match origin {
            Origin::Builtin => self.builtin_artifacts.get(i),
            Origin::Yours => cfg.artifacts.get(i),
        }
    }

    pub fn recipe_rule<'a>(
        &'a self,
        cfg: &'a Config,
        origin: Origin,
        i: usize,
    ) -> Option<&'a RecipeRule> {
        match origin {
            Origin::Builtin => self.builtin_recipes.get(i),
            Origin::Yours => cfg.recipes.get(i),
        }
    }

    /// How many rules a section holds, for the count beside its heading.
    pub fn count(&self, cfg: &Config, section: Section) -> usize {
        match section {
            Section::Roots => cfg.scan.roots.len(),
            Section::Scanning => Setting::ALL.len(),
            Section::Caches => self.builtin_caches.len() + cfg.caches.len(),
            Section::Artifacts => self.builtin_artifacts.len() + cfg.artifacts.len(),
            Section::Ignores => cfg.ignore.len(),
            Section::Overrides => cfg.overrides.len(),
            Section::Recipes => self.builtin_recipes.len() + cfg.recipes.len(),
        }
    }
}

// ---- acting on a row ----------------------------------------------------
//
// Every action below reports `Some(message)` when it changed the config and
// `None` when the key does not apply to the row under the cursor. The caller
// persists on the first and says nothing on the second, so no action can
// change the file without also saying what it did.

impl Settings {
    /// Start typing over the row under the cursor.
    pub fn begin_edit(&mut self, cfg: &Config) -> Option<String> {
        let (target, prompt, buffer) = match self.current()? {
            Row::Root(i) => (
                Target::Root(Some(*i)),
                "directory to search",
                cfg.scan.roots.get(*i)?.clone(),
            ),
            Row::Ignore(i) => (
                Target::Ignore(Some(*i)),
                "pattern never to offer",
                cfg.ignore.get(*i)?.clone(),
            ),
            Row::Cache(Origin::Yours, i) => (
                Target::CachePath(Some(*i)),
                "cache path",
                cfg.caches.get(*i)?.path.clone(),
            ),
            Row::Artifact(Origin::Yours, i) => (
                Target::ArtifactDir(Some(*i)),
                "build directory name",
                cfg.artifacts.get(*i)?.dir.clone(),
            ),
            Row::Setting(setting) if !setting.is_switch() => {
                let setting = *setting;
                let (value, _) = setting.value(cfg);
                (
                    Target::Setting(setting),
                    setting.label(),
                    value.trim_end_matches(" days").to_string(),
                )
            }
            Row::Cache(Origin::Builtin, _) | Row::Artifact(Origin::Builtin, _) => {
                return Some("built-in rules are turned off with x, not edited".into());
            }
            _ => return None,
        };
        self.edit = Some(Edit {
            target,
            prompt: prompt.to_string(),
            buffer,
        });
        None
    }

    /// Rename the rule under the cursor, where it has a name of its own.
    pub fn begin_rename(&mut self, cfg: &Config) -> Option<String> {
        let Row::Cache(Origin::Yours, i) = self.current()? else {
            return None;
        };
        self.edit = Some(Edit {
            target: Target::CacheLabel(*i),
            prompt: "what to call it".to_string(),
            buffer: cfg.caches.get(*i)?.label.clone(),
        });
        None
    }

    /// Start adding to whichever section the cursor is in.
    pub fn begin_add(&mut self) -> Option<String> {
        let section = self.current()?.section();
        let prompt = section.adds()?;
        let target = match section {
            Section::Roots => Target::Root(None),
            Section::Ignores => Target::Ignore(None),
            Section::Caches => Target::CachePath(None),
            Section::Artifacts => Target::ArtifactDir(None),
            _ => return None,
        };
        self.expanded.insert(section);
        self.edit = Some(Edit {
            target,
            prompt: prompt.to_string(),
            buffer: String::new(),
        });
        None
    }

    /// Apply what was typed. `Err` leaves the edit open with the reason.
    pub fn commit_edit(&mut self, cfg: &mut Config) -> Result<String, String> {
        let edit = self.edit.take().ok_or("nothing being edited")?;
        let text = edit.buffer.trim().to_string();
        if text.is_empty() {
            return Err("an empty value would mean nothing — esc cancels".into());
        }

        let outcome = match &edit.target {
            Target::Root(Some(i)) => {
                *cfg.scan.roots.get_mut(*i).ok_or("that root is gone")? = text.clone();
                format!("searching {text}")
            }
            Target::Root(None) => {
                cfg.scan.roots.push(text.clone());
                // Said rather than refused: a directory that does not exist yet
                // is a legitimate thing to configure ahead of creating it, and
                // is also exactly what a typo looks like.
                match crate::config::expand(&text).is_dir() {
                    true => format!("searching {text}"),
                    false => format!("added {text} — nothing there yet"),
                }
            }
            Target::Ignore(Some(i)) => {
                *cfg.ignore.get_mut(*i).ok_or("that pattern is gone")? = text.clone();
                format!("never offering {text}")
            }
            Target::Ignore(None) => {
                if !cfg.add_ignore(text.clone()) {
                    return Err(format!("{text} is already on the list"));
                }
                format!("never offering {text}")
            }
            Target::CachePath(Some(i)) => {
                cfg.caches.get_mut(*i).ok_or("that rule is gone")?.path = text.clone();
                format!("cache path is now {text}")
            }
            Target::CachePath(None) => {
                cfg.caches.push(new_cache_rule(&text));
                format!("added {text} — n renames it, g changes what it costs")
            }
            Target::CacheLabel(i) => {
                cfg.caches.get_mut(*i).ok_or("that rule is gone")?.label = text.clone();
                format!("renamed to {text}")
            }
            Target::ArtifactDir(Some(i)) => {
                cfg.artifacts.get_mut(*i).ok_or("that rule is gone")?.dir = text.clone();
                format!("build directory is now {text}")
            }
            Target::ArtifactDir(None) => {
                if text.contains(['/', '\\']) {
                    self.edit = Some(edit);
                    return Err("a build rule matches one directory name, not a path".into());
                }
                cfg.artifacts.push(ArtifactRule {
                    dir: text.clone(),
                    evidence: Vec::new(),
                    regen: String::new(),
                    risk: RiskName::Rebuildable,
                });
                // Worth saying out loud: with no evidence this matches on the
                // name alone, wherever it appears.
                format!("added {text} — matched by name alone, anywhere it appears")
            }
            Target::Setting(setting) => match apply_setting(cfg, *setting, &text) {
                Ok(message) => message,
                Err(why) => {
                    self.edit = Some(edit);
                    return Err(why);
                }
            },
        };

        self.changed = true;
        self.rebuild(cfg);
        Ok(outcome)
    }

    /// Turn a rule off, or back on.
    pub fn toggle_off(&mut self, cfg: &mut Config) -> Option<String> {
        let pattern = self.off_pattern(cfg)?;
        let message = if is_off(cfg, &pattern) {
            cfg.ignore.retain(|p| *p != pattern);
            format!("{pattern} is offered again")
        } else {
            cfg.add_ignore(pattern.clone());
            format!("never offering {pattern}")
        };
        self.changed = true;
        self.rebuild(cfg);
        Some(message)
    }

    /// The pattern that silences the rule under the cursor.
    pub fn off_pattern(&self, cfg: &Config) -> Option<String> {
        match self.current()? {
            Row::Cache(origin, i) => Some(cache_off_pattern(self.cache_rule(cfg, *origin, *i)?)),
            Row::Artifact(origin, i) => {
                Some(artifact_off_pattern(self.artifact_rule(cfg, *origin, *i)?))
            }
            _ => None,
        }
    }

    /// Move a rule to the next risk grade, or back to what it declares.
    pub fn cycle_grade(&mut self, cfg: &mut Config) -> Option<String> {
        let (pattern, declared) = match self.current()? {
            Row::Cache(origin, i) => {
                let rule = self.cache_rule(cfg, *origin, *i)?;
                (cache_off_pattern(rule), rule.risk)
            }
            Row::Artifact(origin, i) => {
                let rule = self.artifact_rule(cfg, *origin, *i)?;
                (artifact_off_pattern(rule), rule.risk)
            }
            _ => return None,
        };

        let (current, overridden) = effective_risk(cfg, &pattern, declared);
        let next = next_grade(overridden.then_some(current));
        set_override(cfg, &pattern, next);

        self.changed = true;
        self.rebuild(cfg);
        Some(match next {
            Some(risk) => format!("{pattern} now counts as {}", name_of(risk)),
            None => format!("{pattern} is back to {}", name_of(declared)),
        })
    }

    /// Remove something the user added. Built-ins are turned off, never deleted.
    pub fn delete(&mut self, cfg: &mut Config) -> Option<String> {
        let message = match self.current()? {
            Row::Root(i) => format!("no longer searching {}", cfg.scan.roots.remove(*i)),
            Row::Ignore(i) => format!("{} is offered again", cfg.ignore.remove(*i)),
            Row::Override(i) => {
                let removed = cfg.overrides.remove(*i);
                format!("{} is back to its own grading", removed.matches.join(", "))
            }
            Row::Cache(Origin::Yours, i) => format!("removed {}", cfg.caches.remove(*i).label),
            Row::Artifact(Origin::Yours, i) => format!("removed {}", cfg.artifacts.remove(*i).dir),
            Row::Recipe(Origin::Yours, i) => format!("removed {}", cfg.recipes.remove(*i).name),
            Row::Cache(Origin::Builtin, _)
            | Row::Artifact(Origin::Builtin, _)
            | Row::Recipe(Origin::Builtin, _) => {
                return Some("built-in rules are turned off with x, not deleted".into());
            }
            _ => return None,
        };
        self.changed = true;
        self.rebuild(cfg);
        Some(message)
    }

    /// Flip a switch under `[scan]`.
    pub fn toggle_switch(&mut self, cfg: &mut Config) -> Option<String> {
        let Row::Setting(setting) = self.current()? else {
            return None;
        };
        let setting = *setting;
        if !setting.is_switch() {
            return None;
        }
        let now = !setting.is_on(cfg);
        match setting {
            Setting::Docker => cfg.scan.docker = Some(now),
            Setting::Caches => cfg.scan.caches = Some(now),
            Setting::Personal => cfg.scan.personal = Some(now),
            Setting::Trash => cfg.scan.trash = Some(now),
            _ => return None,
        }
        self.changed = true;
        self.rebuild(cfg);
        Some(format!("{} is {}", setting.label(), on_off(now)))
    }
}

/// A rule built from nothing but a path.
///
/// The remaining fields are asked for afterwards, in place, rather than through
/// a form: `n` renames it and `g` says what it costs. One line of typing per
/// question is the whole interaction model of this screen.
fn new_cache_rule(path: &str) -> CacheRule {
    let label = path
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(path);
    CacheRule {
        path: path.to_string(),
        group: "yours".into(),
        label: label.to_string(),
        detail: String::new(),
        // The cautious of the three. A rule someone has not yet said anything
        // about should not be swept up by a recipe that takes everything safe.
        risk: RiskName::Rebuildable,
        prune: Vec::new(),
    }
}

fn name_of(risk: RiskName) -> &'static str {
    match risk {
        RiskName::Safe => "safe",
        RiskName::Rebuildable => "rebuildable",
        RiskName::Irreversible => "irreversible",
    }
}

/// Parse and store one `[scan]` value.
fn apply_setting(cfg: &mut Config, setting: Setting, text: &str) -> Result<String, String> {
    let size = |text: &str| -> Result<String, String> {
        // `parse_size` reads what it can and shrugs at the rest, which is right
        // for a config file read once at startup and wrong for a value someone
        // is watching themselves type.
        if !text.starts_with(|c: char| c.is_ascii_digit()) {
            return Err(format!("{text:?} does not start with a number, e.g. 100MB"));
        }
        if crate::parse_size(text) == 0 && text.trim_start_matches('0') != "" {
            return Err(format!("{text:?} is not a size reap understands"));
        }
        Ok(text.to_string())
    };
    let count = |text: &str| -> Result<u64, String> {
        text.trim_end_matches(" days")
            .trim()
            .parse::<u64>()
            .map_err(|_| format!("{text:?} is not a whole number"))
    };

    let label = setting.label();
    match setting {
        Setting::StaleDays => {
            cfg.scan.stale_days = Some(count(text)?);
            Ok(format!("{label} {text} days"))
        }
        Setting::Depth => {
            cfg.scan.depth = Some(count(text)? as usize);
            Ok(format!("{label} {text} levels"))
        }
        Setting::MinSize => {
            cfg.scan.min_size = Some(size(text)?);
            Ok(format!("{label} {text}"))
        }
        Setting::LibraryCacheFloor => {
            cfg.scan.library_cache_floor = Some(size(text)?);
            Ok(format!("{label} {text}"))
        }
        Setting::DownloadsFloor => {
            cfg.scan.downloads_floor = Some(size(text)?);
            Ok(format!("{label} {text}"))
        }
        _ => Err(format!("{label} is a switch — press space")),
    }
}

/// The ignore pattern that turns a cache rule off.
///
/// A rule that deletes a path is silenced by naming the path, which survives
/// the rule being relabelled. A rule that runs a command has no path to name —
/// `pnpm store prune` is not a removal — so its label is what identifies it.
pub fn cache_off_pattern(rule: &CacheRule) -> String {
    if rule.prune.is_empty() {
        rule.path.clone()
    } else {
        rule.label.clone()
    }
}

/// The ignore pattern that turns an artifact rule off.
///
/// Artifact candidates are labelled by their path, so the directory name has to
/// be matched wherever it appears.
pub fn artifact_off_pattern(rule: &ArtifactRule) -> String {
    format!("*/{}", rule.dir)
}

/// Has this rule been turned off?
pub fn is_off(cfg: &Config, pattern: &str) -> bool {
    cfg.ignore.iter().any(|p| p == pattern)
}

/// The risk a rule carries once any override the user wrote is applied.
pub fn effective_risk(cfg: &Config, pattern: &str, declared: RiskName) -> (RiskName, bool) {
    match cfg
        .overrides
        .iter()
        .filter(|o| o.matches.iter().any(|m| m == pattern))
        .next_back()
    {
        Some(o) => (o.risk, true),
        None => (declared, false),
    }
}

/// safe → rebuildable → irreversible → back to whatever the rule declared.
///
/// Cycling off the end removes the override rather than pinning the declared
/// value, so a rule re-graded and then put back is indistinguishable from one
/// never touched — and picks up a corrected default in a later release.
pub fn next_grade(current: Option<RiskName>) -> Option<RiskName> {
    match current {
        None => Some(RiskName::Safe),
        Some(RiskName::Safe) => Some(RiskName::Rebuildable),
        Some(RiskName::Rebuildable) => Some(RiskName::Irreversible),
        Some(RiskName::Irreversible) => None,
    }
}

/// Write, replace or drop the override naming `pattern`.
pub fn set_override(cfg: &mut Config, pattern: &str, risk: Option<RiskName>) {
    cfg.overrides
        .retain(|o| !(o.matches.len() == 1 && o.matches[0] == pattern));
    if let Some(risk) = risk {
        cfg.overrides.push(OverrideRule {
            matches: vec![pattern.to_string()],
            risk,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_cache(path: &str, label: &str, prune: &[&str]) -> CacheRule {
        CacheRule {
            path: path.into(),
            group: "test".into(),
            label: label.into(),
            detail: String::new(),
            risk: RiskName::Safe,
            prune: prune.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    #[test]
    fn a_rule_that_deletes_a_path_is_turned_off_by_naming_the_path() {
        // Which survives the rule being relabelled in a later release.
        let rule = a_cache("~/.cache/thing", "Thing cache", &[]);
        assert_eq!(cache_off_pattern(&rule), "~/.cache/thing");
    }

    #[test]
    fn a_rule_that_runs_a_command_is_turned_off_by_naming_its_label() {
        // `pnpm store prune` is not a removal, so the candidate carries no path
        // for an ignore to match against.
        let rule = a_cache("~/Library/pnpm/store", "pnpm store", &["pnpm", "store"]);
        assert_eq!(cache_off_pattern(&rule), "pnpm store");
    }

    #[test]
    fn an_artifact_rule_is_turned_off_wherever_its_directory_appears() {
        let rule = ArtifactRule {
            dir: "node_modules".into(),
            evidence: vec![],
            regen: String::new(),
            risk: RiskName::Rebuildable,
        };
        assert_eq!(artifact_off_pattern(&rule), "*/node_modules");
    }

    #[test]
    fn grading_cycles_through_every_level_and_back_to_the_default() {
        // The last step removes the override rather than pinning the value, so
        // a rule put back is indistinguishable from one never touched.
        assert_eq!(next_grade(None), Some(RiskName::Safe));
        assert_eq!(
            next_grade(Some(RiskName::Safe)),
            Some(RiskName::Rebuildable)
        );
        assert_eq!(
            next_grade(Some(RiskName::Rebuildable)),
            Some(RiskName::Irreversible)
        );
        assert_eq!(next_grade(Some(RiskName::Irreversible)), None);
    }

    #[test]
    fn re_grading_the_same_rule_twice_leaves_one_override() {
        // Otherwise the file grows a line per keystroke, and the last-one-wins
        // rule makes every earlier line dead weight nobody can interpret.
        let mut cfg = Config::default();
        set_override(&mut cfg, "~/.cache/thing", Some(RiskName::Safe));
        set_override(&mut cfg, "~/.cache/thing", Some(RiskName::Irreversible));

        assert_eq!(cfg.overrides.len(), 1);
        assert_eq!(cfg.overrides[0].risk, RiskName::Irreversible);

        set_override(&mut cfg, "~/.cache/thing", None);
        assert!(cfg.overrides.is_empty());
    }

    #[test]
    fn re_grading_leaves_an_override_a_person_wrote_by_hand_alone() {
        // A hand-written rule covering several patterns is a decision reap did
        // not make and must not quietly rewrite.
        let mut cfg = Config::default();
        cfg.overrides.push(OverrideRule {
            matches: vec!["caches/*".into(), "docker/*".into()],
            risk: RiskName::Safe,
        });
        set_override(&mut cfg, "caches/*", Some(RiskName::Irreversible));

        assert_eq!(cfg.overrides.len(), 2, "the hand-written rule must survive");
        assert_eq!(cfg.overrides[0].matches.len(), 2);
    }

    #[test]
    fn the_effective_risk_is_the_last_override_that_names_the_rule() {
        let mut cfg = Config::default();
        assert_eq!(
            effective_risk(&cfg, "~/x", RiskName::Safe),
            (RiskName::Safe, false)
        );

        set_override(&mut cfg, "~/x", Some(RiskName::Irreversible));
        assert_eq!(
            effective_risk(&cfg, "~/x", RiskName::Safe),
            (RiskName::Irreversible, true)
        );
    }

    #[test]
    fn every_section_is_reachable_and_every_row_knows_its_section() {
        let cfg = Config::default();
        let mut settings = Settings::new(&cfg);
        settings.expanded = Section::ALL.into_iter().collect();
        settings.rebuild(&cfg);

        for section in Section::ALL {
            assert!(
                settings.rows.contains(&Row::Heading(section)),
                "no heading for {section:?}"
            );
        }
        for row in &settings.rows {
            // The footer decides which keys apply from this, so a row whose
            // section is wrong offers the wrong actions.
            assert!(Section::ALL.contains(&row.section()));
        }
    }

    #[test]
    fn a_collapsed_section_shows_only_its_heading() {
        // Ninety cache rules expanded by default would bury every other
        // section below the fold.
        let cfg = Config::default();
        let settings = Settings::new(&cfg);

        assert!(!settings.expanded.contains(&Section::Caches));
        assert!(
            !settings
                .rows
                .iter()
                .any(|r| matches!(r, Row::Cache(..) | Row::Add(Section::Caches))),
            "a collapsed section must contribute no rows of its own"
        );
        assert!(settings.rows.contains(&Row::Heading(Section::Caches)));
    }

    #[test]
    fn the_counts_beside_a_heading_include_both_origins() {
        let mut cfg = Config::default();
        let settings = Settings::new(&cfg);
        let builtin = settings.count(&cfg, Section::Caches);
        assert!(builtin > 0, "reap ships with cache rules");

        cfg.caches.push(a_cache("~/mine", "mine", &[]));
        assert_eq!(settings.count(&cfg, Section::Caches), builtin + 1);
    }

    #[test]
    fn a_setting_says_whether_its_value_was_chosen_or_defaulted_to() {
        // The difference is the whole reason the column exists: a person
        // looking at this screen is trying to find out what they changed.
        let mut cfg = Config::default();
        assert_eq!(
            Setting::StaleDays.value(&cfg),
            ("30 days".into(), Origin::Builtin)
        );

        cfg.scan.stale_days = Some(90);
        assert_eq!(
            Setting::StaleDays.value(&cfg),
            ("90 days".into(), Origin::Yours)
        );
    }

    #[test]
    fn a_switch_is_on_unless_it_was_turned_off() {
        let mut cfg = Config::default();
        assert!(Setting::Personal.is_on(&cfg));
        assert_eq!(
            Setting::Personal.value(&cfg),
            ("on".into(), Origin::Builtin)
        );

        cfg.scan.personal = Some(false);
        assert!(!Setting::Personal.is_on(&cfg));
        assert_eq!(Setting::Personal.value(&cfg), ("off".into(), Origin::Yours));

        // Trashing is the one that is off until asked for.
        assert!(!Setting::Trash.is_on(&Config::default()));
    }

    #[test]
    fn only_the_sections_that_take_additions_offer_one() {
        let cfg = Config::default();
        let mut settings = Settings::new(&cfg);
        settings.expanded = Section::ALL.into_iter().collect();
        settings.rebuild(&cfg);

        for section in Section::ALL {
            let offered = settings.rows.contains(&Row::Add(section));
            assert_eq!(
                offered,
                section.adds().is_some(),
                "{section:?} offers add: {offered}, but adds() says otherwise"
            );
        }
        // Nothing can be added to a list reap generates or a screen of values.
        assert!(Section::Overrides.adds().is_none());
        assert!(Section::Scanning.adds().is_none());
    }
}
