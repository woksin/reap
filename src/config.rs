//! User configuration: `~/.config/reap/config.toml`.
//!
//! Everything the scanners know about — which directories count as build
//! output, which caches are worth offering, what to never descend into — comes
//! from here, seeded with built-in defaults. Nothing requires a recompile.

use crate::model::Risk;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Deserialize, Serialize, Default, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub scan: ScanSection,

    /// Candidates never to offer. Matched against the path, the label, and
    /// `category/group`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ignore: Vec<String>,

    /// Extra directory names the walk refuses to descend into.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub never_descend: Vec<String>,

    /// Replace the built-in artifact rules entirely rather than adding to them.
    #[serde(skip_serializing_if = "is_false")]
    pub replace_builtin_artifacts: bool,
    /// Replace the built-in cache rules entirely rather than adding to them.
    #[serde(skip_serializing_if = "is_false")]
    pub replace_builtin_caches: bool,
    /// Replace the built-in quick-reap recipes entirely rather than adding to
    /// them. A user whose work does not look like the built-in assumptions
    /// wants their own keys, not their own keys plus mine.
    #[serde(skip_serializing_if = "is_false")]
    pub replace_builtin_recipes: bool,

    // Tables must follow every top-level scalar in TOML, so these are declared
    // last: serde emits fields in order, and a file written here has to stay
    // valid when a user appends another `[[artifact]]` by hand.
    #[serde(rename = "artifact", skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactRule>,
    #[serde(rename = "cache", skip_serializing_if = "Vec::is_empty")]
    pub caches: Vec<CacheRule>,
    #[serde(rename = "recipe", skip_serializing_if = "Vec::is_empty")]
    pub recipes: Vec<RecipeRule>,
    #[serde(rename = "override", skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<OverrideRule>,
}

#[derive(Deserialize, Serialize, Default, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct ScanSection {
    /// Where to look for repositories and build artifacts.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub roots: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_days: Option<u64>,
    /// Accepts a suffix, e.g. "50MB".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<usize>,
    /// Report unnamed application-cache entries at least this large.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_cache_floor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trash: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docker: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caches: Option<bool>,
}

/// A directory that a build tool regenerates.
#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRule {
    /// Directory name to match.
    pub dir: String,
    /// Sibling files proving what it is. `*.ext` matches by extension; an empty
    /// list means the name alone is unambiguous.
    #[serde(default)]
    pub evidence: Vec<String>,
    /// What regenerates it, shown in the detail line.
    #[serde(default)]
    pub regen: String,
    #[serde(default = "default_risk")]
    pub risk: RiskName,
}

/// A cache directory and how to clear it properly.
#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct CacheRule {
    /// Path, `~` accepted.
    pub path: String,
    #[serde(default)]
    pub group: String,
    pub label: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default = "default_risk")]
    pub risk: RiskName,
    /// Command to run instead of deleting, for tools that keep their own
    /// bookkeeping — `["pnpm", "store", "prune"]`.
    #[serde(default)]
    pub prune: Vec<String>,
}

/// Re-grade what reap considers something to cost.
///
/// The built-in risk levels are one person's judgement about what is expensive
/// to lose, and that judgement does not survive contact with everyone's setup.
/// A cache someone re-downloads over a fast link is safe to them; a stopped
/// container someone is keeping to debug is not. Since risk is what `s` and
/// the recipes select by, being able to correct it is what makes those keys
/// fit rather than nearly fit.
#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct OverrideRule {
    /// What it applies to. Matched against the path, the label and
    /// `category/group`, exactly as `ignore` is.
    #[serde(rename = "match")]
    pub matches: Vec<String>,
    /// The risk to give it instead.
    pub risk: RiskName,
}

/// A one-key selection: "everything docker can spare", "the branches that are
/// already upstream".
///
/// The work of using reap is deciding what to tick, and that decision is nearly
/// always the same one. A recipe is that decision written down once — matched
/// the same way `ignore` is, so a pattern learned in one place works in the
/// other.
#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct RecipeRule {
    /// The key that runs it. Single character.
    pub key: char,
    /// Shown in the palette.
    pub name: String,
    /// What it covers. Matched against the path, the label and
    /// `category/group`, exactly as `ignore` is. Empty means everything.
    #[serde(default, rename = "match")]
    pub matches: Vec<String>,
    /// The most dangerous thing it will tick. `safe` never selects anything
    /// that costs a rebuild; `irreversible` selects whatever the patterns
    /// cover, and still has to get past the typed confirmation.
    #[serde(default = "default_recipe_risk")]
    pub max_risk: RiskName,
    /// One line on what it leaves behind, shown under the palette.
    #[serde(default)]
    pub detail: String,
}

#[derive(Deserialize, Serialize, Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum RiskName {
    Safe,
    Rebuildable,
    Irreversible,
}

fn default_recipe_risk() -> RiskName {
    RiskName::Safe
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn default_risk() -> RiskName {
    RiskName::Rebuildable
}

impl From<RiskName> for Risk {
    fn from(r: RiskName) -> Risk {
        match r {
            RiskName::Safe => Risk::Safe,
            RiskName::Rebuildable => Risk::Caution,
            RiskName::Irreversible => Risk::Danger,
        }
    }
}

impl From<Risk> for RiskName {
    fn from(r: Risk) -> RiskName {
        match r {
            Risk::Safe => RiskName::Safe,
            Risk::Caution => RiskName::Rebuildable,
            Risk::Danger => RiskName::Irreversible,
        }
    }
}

/// `~/.config/reap/config.toml`, honouring `XDG_CONFIG_HOME`.
pub fn default_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| crate::scan::home_dir().map(|h| h.join(".config")))?;
    Some(base.join("reap").join("config.toml"))
}

/// Expand a leading `~` against `$HOME`.
pub fn expand(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/")
        && let Some(home) = crate::scan::home_dir()
    {
        return home.join(rest);
    }
    if raw == "~"
        && let Some(home) = crate::scan::home_dir()
    {
        return home;
    }
    PathBuf::from(raw)
}

#[derive(Debug)]
pub enum LoadError {
    Read(std::io::Error),
    Parse(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Read(e) => write!(f, "{e}"),
            LoadError::Parse(e) => write!(f, "{e}"),
        }
    }
}

impl Config {
    /// Load from `path`. A missing file is not an error — it means defaults.
    /// A malformed one is, because silently ignoring it would quietly change
    /// which files get deleted.
    pub fn load(path: &Path) -> Result<Self, LoadError> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| LoadError::Parse(e.to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(LoadError::Read(e)),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        std::fs::write(path, format!("{CONFIG_HEADER}{body}"))
    }

    /// Add an ignore pattern if it is not already present.
    pub fn add_ignore(&mut self, pattern: String) -> bool {
        if self.ignore.contains(&pattern) {
            return false;
        }
        self.ignore.push(pattern);
        true
    }

    /// Write a documented starter file.
    ///
    /// Deliberately a hand-written template rather than a serialised default:
    /// it can carry commented examples, and it puts the tables last so that
    /// appending another `[[artifact]]` cannot swallow a top-level key.
    pub fn write_template(path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, TEMPLATE)
    }
}

const TEMPLATE: &str = r##"# reap configuration
#
# Every value here is optional, and command-line flags override it.
# Delete anything you do not need.

# ---------------------------------------------------------------------------
# Never offer these
# ---------------------------------------------------------------------------
# Patterns are matched against a candidate's path, its label, and its
# "category/group". `*` matches any run of characters, and a pattern with no
# wildcard also matches everything beneath it.
#
# The `x` key in the interface appends to this list.
ignore = [
  # "~/.nuget/packages",          # one cache, always
  # "*/vendor",                   # any vendor directory, anywhere
  # "git/unpushed branches",      # a whole group
  # "docker/unused volumes",
]

# Directory names the scan refuses to descend into. Added to the built-in list
# (node_modules, target, Library, ...).
never_descend = [
  # "Games",
]

# Use only the rules below, ignoring everything reap ships with.
# replace_builtin_artifacts = false
# replace_builtin_caches = false
# replace_builtin_recipes = false

# ---------------------------------------------------------------------------
# Scanning
# ---------------------------------------------------------------------------
[scan]
# Where to look. Defaults to the usual suspects under $HOME:
# repos, src, Developer, Projects, code, dev, work, git.
# roots = ["~/work", "~/oss"]

# stale_days = 30                # untouched this long before it counts as stale
# min_size = "1MB"               # hide anything smaller
# depth = 8                      # how deep to descend from each root
# library_cache_floor = "200MB"  # floor for unnamed application caches
# trash = false                  # move to the Trash instead of deleting
# docker = true                  # set false to skip the Docker scan
# caches = true                  # set false to skip the cache scan

# ---------------------------------------------------------------------------
# Extra build-artifact directories
# ---------------------------------------------------------------------------
# `evidence` is the sibling files that prove what a directory is — without it
# any directory sharing the name would match. `*.ext` matches by extension.
# risk: safe | rebuildable | irreversible
#
# [[artifact]]
# dir = "my-build-output"
# evidence = ["Makefile"]
# regen = "make"
# risk = "rebuildable"

# ---------------------------------------------------------------------------
# Extra caches
# ---------------------------------------------------------------------------
# `prune` runs a command instead of deleting the path, for tools that keep
# their own bookkeeping. Omit it for a plain removal.
#
# [[cache]]
# path = "~/.cache/my-tool"
# group = "package managers"
# label = "my-tool cache"
# detail = "re-downloaded on next run"
# risk = "safe"
# prune = ["my-tool", "cache", "clean"]

# ---------------------------------------------------------------------------
# Re-grade what something costs you
# ---------------------------------------------------------------------------
# The built-in risk levels are one person's judgement. A cache you re-download
# over a fast link is safe to you; a stopped container you are keeping to debug
# is not. Risk is what `s` and the recipes select by, so correcting it is what
# makes those keys fit rather than nearly fit.
#
# `match` uses the same patterns as `ignore`. The last matching rule wins, so
# write the broad one first and carve exceptions out below it. Ignoring beats
# re-grading: something you said never to offer stays unoffered.
#
# [[override]]
# match = ["caches/*"]
# risk = "safe"
#
# [[override]]
# match = ["~/.cache/precious"]
# risk = "irreversible"

# ---------------------------------------------------------------------------
# Quick-reap recipes — the R key
# ---------------------------------------------------------------------------
# One key for a decision you make the same way every time. A recipe only
# selects; the confirm dialog still gates what happens next, so a recipe can
# never delete something ticking by hand would not have.
#
# `match` uses the same patterns as `ignore` above. Leave it out to mean
# everything, and let `max_risk` do the bounding.
#
# max_risk is the most dangerous thing it will tick:
#   safe          nothing that costs a rebuild
#   rebuildable   time, but no work
#   irreversible  whatever the patterns cover
#
# These add to the built-in recipes. A key you reuse takes that key over.
#
# [[recipe]]
# key = "n"
# name = "Node · every node_modules"
# detail = "pnpm install brings them all back"
# match = ["build artifacts/node_modules"]
# max_risk = "rebuildable"
#
# [[recipe]]
# key = "p"
# name = "This project only"
# match = ["~/work/big-monorepo/*"]
# max_risk = "rebuildable"
"##;

const CONFIG_HEADER: &str = "\
# reap configuration.
#
# `ignore` patterns are matched against a candidate's path, its label, and its
# `category/group`. `*` matches any run of characters, and a plain path also
# matches everything beneath it:
#
#   ignore = [
#     \"~/.nuget/packages\",      # this cache, always
#     \"*/vendor\",               # any vendor directory
#     \"docker/unused volumes\",  # a whole group
#     \"caches/application caches\",
#   ]
#
# `[[artifact]]` and `[[cache]]` entries add to the built-in rules. Set
# `replace_builtin_artifacts` or `replace_builtin_caches` to use only yours.
#
# risk is one of: safe, rebuildable, irreversible

";

/// Compiled ignore patterns.
#[derive(Default)]
pub struct IgnoreSet {
    /// (original pattern, expanded form used for matching)
    patterns: Vec<(String, String)>,
}

impl IgnoreSet {
    pub fn new(patterns: &[String]) -> Self {
        Self {
            patterns: patterns
                .iter()
                .map(|p| {
                    let expanded = if p.starts_with('~') {
                        expand(p).to_string_lossy().into_owned()
                    } else {
                        p.clone()
                    };
                    (p.clone(), expanded)
                })
                .collect(),
        }
    }

    /// True when any pattern matches the given text.
    pub fn matches_text(&self, text: &str) -> bool {
        self.patterns
            .iter()
            .any(|(raw, expanded)| matches_one(expanded, text) || matches_one(raw, text))
    }

    pub fn matches_path(&self, path: &Path) -> bool {
        let s = path.to_string_lossy();
        self.matches_text(&s) || self.matches_text(&crate::util::tilde(path))
    }

    /// Everything a candidate can be ignored by.
    pub fn matches_candidate(&self, cand: &crate::model::Candidate) -> bool {
        if self.patterns.is_empty() {
            return false;
        }
        if let crate::model::Action::Remove(p) = &cand.action
            && self.matches_path(p)
        {
            return true;
        }
        let cat = cand.category.title().to_lowercase();
        self.matches_text(&cand.label)
            || self.matches_text(&format!("{cat}/{}", cand.group))
            || self.matches_text(&cand.group)
    }
}

/// Match `text` against a pattern where `*` stands for any run of characters.
///
/// A pattern with no wildcard also matches anything beneath it, so
/// `~/.nuget/packages` covers the directory and its contents.
fn matches_one(pattern: &str, text: &str) -> bool {
    if !pattern.contains('*') {
        return text == pattern
            || text
                .strip_prefix(pattern)
                .is_some_and(|r| r.starts_with('/'));
    }

    // Iterative two-pointer glob with backtracking: linear in practice and
    // immune to the pathological recursion a naive version has.
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (usize::MAX, 0usize);

    while ti < t.len() {
        if pi < p.len() && (p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = pi;
            mark = ti;
            pi += 1;
        } else if star != usize::MAX {
            pi = star + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_pattern_covers_the_path_and_its_contents() {
        assert!(matches_one("/a/b", "/a/b"));
        assert!(matches_one("/a/b", "/a/b/c/d"));
        // Must not match a sibling that merely shares a prefix.
        assert!(!matches_one("/a/b", "/a/bc"));
        assert!(!matches_one("/a/b", "/a"));
    }

    #[test]
    fn wildcards_match_any_run() {
        assert!(matches_one("*/vendor", "/home/me/proj/vendor"));
        assert!(matches_one("*node_modules*", "/x/node_modules/y"));
        assert!(matches_one("docker/*", "docker/unused volumes"));
        assert!(matches_one("*", "anything"));
        assert!(!matches_one("*/vendor", "/home/me/vendors"));
    }

    #[test]
    fn trailing_stars_are_optional() {
        assert!(matches_one("abc*", "abc"));
        assert!(matches_one("a*c", "abbbc"));
        assert!(!matches_one("a*c", "abbbd"));
    }

    fn candidate(label: &str, group: &str, path: Option<&str>) -> crate::model::Candidate {
        use crate::model::{Action, Candidate, Category};
        Candidate::new(
            Category::Caches,
            group,
            label,
            "",
            0,
            Risk::Safe,
            match path {
                Some(p) => Action::Remove(PathBuf::from(p)),
                None => Action::Run {
                    program: "true".into(),
                    args: vec![],
                    cwd: None,
                },
            },
        )
    }

    #[test]
    fn ignores_by_path_label_or_group() {
        let set = IgnoreSet::new(&[
            "/opt/keep".to_string(),
            "JetBrains".to_string(),
            "caches/application caches".to_string(),
        ]);

        assert!(set.matches_candidate(&candidate("x", "g", Some("/opt/keep/sub"))));
        assert!(set.matches_candidate(&candidate("JetBrains", "g", None)));
        assert!(set.matches_candidate(&candidate("x", "application caches", None)));
        assert!(!set.matches_candidate(&candidate("x", "g", Some("/opt/other"))));
    }

    #[test]
    fn an_empty_set_ignores_nothing() {
        let set = IgnoreSet::new(&[]);
        assert!(!set.matches_candidate(&candidate("anything", "any", Some("/a/b"))));
    }

    #[test]
    fn round_trips_through_toml() {
        let mut c = Config::default();
        c.scan.roots = vec!["~/work".into()];
        c.scan.stale_days = Some(90);
        c.add_ignore("~/.nuget/packages".into());
        c.artifacts.push(ArtifactRule {
            dir: ".cache".into(),
            evidence: vec!["package.json".into()],
            regen: "the build".into(),
            risk: RiskName::Safe,
        });

        let text = toml::to_string_pretty(&c).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.scan.roots, ["~/work"]);
        assert_eq!(back.scan.stale_days, Some(90));
        assert_eq!(back.ignore, ["~/.nuget/packages"]);
        assert_eq!(back.artifacts[0].dir, ".cache");
        assert_eq!(back.artifacts[0].risk, RiskName::Safe);
    }

    #[test]
    fn a_missing_file_means_defaults() {
        let c = Config::load(Path::new("/nonexistent/reap/config.toml")).unwrap();
        assert!(c.ignore.is_empty());
        assert!(c.artifacts.is_empty());
    }

    #[test]
    fn a_malformed_file_is_an_error_not_a_silent_default() {
        let p = std::env::temp_dir().join(format!("reap-badcfg-{}.toml", std::process::id()));
        std::fs::write(&p, "this is not = = toml").unwrap();
        // Ignoring a broken config would quietly change which files get deleted.
        assert!(Config::load(&p).is_err());
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn unknown_keys_are_rejected_rather_than_ignored() {
        // A typo in an ignore rule must not silently do nothing.
        let err = toml::from_str::<Config>("ignoer = [\"x\"]");
        assert!(err.is_err(), "unknown key should be rejected");
    }

    #[test]
    fn duplicate_ignores_are_not_added_twice() {
        let mut c = Config::default();
        assert!(c.add_ignore("a".into()));
        assert!(!c.add_ignore("a".into()));
        assert_eq!(c.ignore.len(), 1);
    }
}

#[cfg(test)]
mod template_tests {
    use super::*;

    #[test]
    fn the_starter_template_parses() {
        let cfg: Config = toml::from_str(TEMPLATE).expect("template must be valid TOML");
        assert!(cfg.ignore.is_empty());
        assert!(cfg.artifacts.is_empty());
    }

    #[test]
    fn the_override_the_template_shows_is_one_that_works() {
        let uncommented: String = TEMPLATE
            .lines()
            .skip_while(|l| !l.contains("[[override]]"))
            // Stops at the rule that opens the next section, which is not TOML.
            .take_while(|l| !l.starts_with("# ---"))
            .map(|l| l.trim_start_matches("# ").trim_start_matches('#'))
            .collect::<Vec<_>>()
            .join("\n");

        let cfg: Config =
            toml::from_str(&uncommented).expect("the template's own override example must parse");
        assert_eq!(cfg.overrides.len(), 2);
        assert_eq!(cfg.overrides[0].risk, RiskName::Safe);
        assert_eq!(cfg.overrides[1].risk, RiskName::Irreversible);
    }

    #[test]
    fn the_recipe_the_template_shows_is_one_that_works() {
        // A commented example that does not parse when uncommented is worse
        // than no example: a malformed config is fatal, so the first thing a
        // user tries would stop reap from starting.
        let uncommented: String = TEMPLATE
            .lines()
            .skip_while(|l| !l.contains("[[recipe]]"))
            .map(|l| l.trim_start_matches("# ").trim_start_matches('#'))
            .collect::<Vec<_>>()
            .join("\n");

        let cfg: Config =
            toml::from_str(&uncommented).expect("the template's own recipe example must parse");
        assert_eq!(cfg.recipes.len(), 2, "both examples should be there");
        assert_eq!(cfg.recipes[0].key, 'n');

        // And it has to survive compiling, not merely deserialising.
        let compiled = crate::recipes::compile(&cfg);
        assert!(compiled.iter().any(|r| r.key == 'n'));
    }

    #[test]
    fn a_hand_appended_table_does_not_swallow_top_level_keys() {
        // The trap this template exists to avoid: TOML tables must follow every
        // top-level scalar, so appending a rule has to stay valid.
        let appended = format!(
            "{TEMPLATE}\n[[artifact]]\ndir = \"out\"\nevidence = [\"Makefile\"]\nrisk = \"safe\"\n"
        );
        let cfg: Config = toml::from_str(&appended).expect("appending a rule must stay valid");
        assert_eq!(cfg.artifacts.len(), 1);
        assert_eq!(cfg.artifacts[0].dir, "out");
    }

    #[test]
    fn a_saved_config_can_be_appended_to() {
        // What `x` writes must survive the same treatment.
        let mut cfg = Config::default();
        cfg.add_ignore("~/.nuget/packages".into());
        cfg.scan.stale_days = Some(90);

        let dir = std::env::temp_dir().join(format!("reap-save-{}", std::process::id()));
        let path = dir.join("config.toml");
        cfg.save(&path).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        let appended = format!("{text}\n[[cache]]\npath = \"~/x\"\nlabel = \"x\"\n");
        let back: Config = toml::from_str(&appended).expect("saved config must stay appendable");
        assert_eq!(back.ignore, ["~/.nuget/packages"]);
        assert_eq!(back.scan.stale_days, Some(90));
        assert_eq!(back.caches.len(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }
}
