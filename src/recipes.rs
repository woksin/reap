//! Quick reaps: one key for a decision you make the same way every time.
//!
//! Ticking items is the work of using reap, and after the first few runs it is
//! the same work — everything docker can spare, the branches already upstream,
//! the build output of whichever project is cold. A recipe is that decision
//! named and bound to a key.
//!
//! A recipe only ever *selects*. It cannot delete anything the confirm dialog
//! would not have gated anyway, so a one-key selection of two hundred items is
//! no more dangerous than ticking them by hand — you still see the risk split
//! and still type `reap` if anything irreversible is in there.

use crate::config::{Config, IgnoreSet, RecipeRule, RiskName};
use crate::model::{Candidate, Risk};

/// A recipe with its patterns compiled.
pub struct Recipe {
    pub key: char,
    pub name: String,
    pub detail: String,
    pub max_risk: Risk,
    /// Empty means the recipe is bounded by risk alone.
    patterns: IgnoreSet,
    unbounded: bool,
}

impl Recipe {
    /// Whether this recipe would tick `cand`.
    pub fn covers(&self, cand: &Candidate) -> bool {
        cand.risk <= self.max_risk && (self.unbounded || self.patterns.matches_candidate(cand))
    }
}

/// The user's recipes, on top of the built-ins unless they replaced them.
///
/// A later recipe wins a contested key, so a user's own `d` overrides the
/// built-in `d` rather than being unreachable behind it.
pub fn compile(cfg: &Config) -> Vec<Recipe> {
    let mut rules: Vec<RecipeRule> = if cfg.replace_builtin_recipes {
        Vec::new()
    } else {
        builtin()
    };
    rules.extend(cfg.recipes.iter().cloned());

    let mut out: Vec<Recipe> = Vec::new();
    for rule in rules {
        let recipe = Recipe {
            key: rule.key,
            name: rule.name,
            detail: rule.detail,
            max_risk: rule.max_risk.into(),
            unbounded: rule.matches.is_empty(),
            patterns: IgnoreSet::new(&rule.matches),
        };
        match out.iter().position(|r| r.key == recipe.key) {
            Some(i) => out[i] = recipe,
            None => out.push(recipe),
        }
    }
    out
}

/// The recipes reap ships with.
///
/// Chosen for the way a machine actually fills up when work is spread across
/// many short-lived branches and worktrees: the same four or five decisions,
/// over and over, in the same order of nerve.
pub fn builtin() -> Vec<RecipeRule> {
    let r =
        |key: char, name: &str, detail: &str, max_risk: RiskName, matches: &[&str]| RecipeRule {
            key,
            name: name.into(),
            detail: detail.into(),
            max_risk,
            matches: matches.iter().map(|s| (*s).to_string()).collect(),
        };

    vec![
        // The three broad strokes, ordered by nerve, on the number row so they
        // read as a scale.
        r(
            '1',
            "Everything safe",
            "regenerated automatically · nothing is lost",
            RiskName::Safe,
            &[],
        ),
        r(
            '2',
            "Everything but the irreversible",
            "costs a rebuild or a re-download · no work is lost",
            RiskName::Rebuildable,
            &[],
        ),
        r(
            '3',
            "Absolutely everything",
            "includes work that exists nowhere else · confirm by typing reap",
            RiskName::Irreversible,
            &[],
        ),
        // Then per-tool, on the letter each tool is already called by.
        r(
            'g',
            "Git · branches already upstream",
            "merged and squash-merged · already in the integration branch",
            RiskName::Safe,
            &["git/merged branches", "git/squash-merged branches"],
        ),
        r(
            'w',
            "Git · worktrees with nothing in them",
            "nothing uncommitted, nothing unpushed · only the checkout goes",
            RiskName::Rebuildable,
            &["git/prunable worktrees"],
        ),
        r(
            'G',
            "Git · everything it can spare",
            "branches, worktrees, stashes, repacking · never an unpushed commit",
            RiskName::Rebuildable,
            &["git/*"],
        ),
        r(
            'b',
            "Build artifacts",
            "every target, node_modules and bin your build system can remake",
            RiskName::Rebuildable,
            &["build artifacts/*"],
        ),
        r(
            'd',
            "Docker · safe",
            "dangling images, reclaimable build cache, unused networks",
            RiskName::Safe,
            &["docker/*"],
        ),
        r(
            'D',
            "Docker · everything but the volumes",
            "also unused images and stopped containers · volumes stay put",
            RiskName::Rebuildable,
            &["docker/*"],
        ),
        r(
            'c',
            "Caches",
            "package managers and tool caches · re-downloaded on next use",
            RiskName::Rebuildable,
            &["caches/*"],
        ),
        // And the ones for a machine that is not primarily a build machine.
        // `c` covers these too, but it also covers nine gigabytes of NuGet
        // packages — which is the right answer for a developer and a
        // bewildering one for anybody else.
        r(
            'a',
            "Apps · what they will simply rebuild",
            "chat, browsers, design tools · nothing you are signed into is lost",
            RiskName::Safe,
            &[
                "caches/app caches",
                "caches/application caches",
                "caches/web browsers",
                "caches/creative tools",
                "caches/media apps",
                "caches/system",
            ],
        ),
        r(
            'i',
            "Installers you have already run",
            "disk images and setup files · the apps they installed stay installed",
            RiskName::Rebuildable,
            &["personal/installers"],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Action, Category};

    fn candidate(cat: Category, group: &str, risk: Risk) -> Candidate {
        Candidate::new(
            cat,
            group,
            "label",
            "",
            1,
            risk,
            Action::Run {
                program: "true".into(),
                args: vec![],
                cwd: None,
            },
        )
    }

    #[test]
    fn every_builtin_key_is_distinct() {
        // Two recipes on one key would make the second unreachable.
        let mut keys: Vec<char> = builtin().iter().map(|r| r.key).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(
            keys.len(),
            before,
            "duplicate key among the built-in recipes"
        );
    }

    #[test]
    fn no_builtin_key_collides_with_the_palettes_own_keys() {
        // These leave the palette rather than run a recipe.
        for reserved in ['q', '?'] {
            assert!(
                !builtin().iter().any(|r| r.key == reserved),
                "recipe bound to {reserved}, which closes the palette"
            );
        }
    }

    #[test]
    fn a_recipe_is_bounded_by_risk_as_well_as_by_pattern() {
        let recipes = compile(&Config::default());
        let docker_safe = recipes.iter().find(|r| r.key == 'd').unwrap();

        assert!(docker_safe.covers(&candidate(Category::Docker, "build cache", Risk::Safe)));
        // A stopped container is docker, but it is not safe.
        assert!(!docker_safe.covers(&candidate(
            Category::Docker,
            "stopped containers",
            Risk::Caution
        )));
        // And git is not docker at any risk.
        assert!(!docker_safe.covers(&candidate(Category::Git, "merged branches", Risk::Safe)));
    }

    #[test]
    fn an_unbounded_recipe_is_bounded_by_risk_alone() {
        let recipes = compile(&Config::default());
        let safe = recipes.iter().find(|r| r.key == '1').unwrap();

        assert!(safe.covers(&candidate(Category::Git, "merged branches", Risk::Safe)));
        assert!(safe.covers(&candidate(Category::Caches, "npm", Risk::Safe)));
        assert!(!safe.covers(&candidate(Category::Caches, "npm", Risk::Caution)));
    }

    #[test]
    fn a_configured_recipe_can_take_over_a_built_in_key() {
        let mut cfg = Config::default();
        cfg.recipes.push(RecipeRule {
            key: 'd',
            name: "mine".into(),
            detail: String::new(),
            max_risk: RiskName::Irreversible,
            matches: vec!["docker/unused volumes".into()],
        });
        let recipes = compile(&cfg);

        assert_eq!(recipes.iter().filter(|r| r.key == 'd').count(), 1);
        let mine = recipes.iter().find(|r| r.key == 'd').unwrap();
        assert_eq!(mine.name, "mine");
        assert!(mine.covers(&candidate(Category::Docker, "unused volumes", Risk::Danger)));
    }

    #[test]
    fn replacing_the_built_ins_leaves_only_the_users_own() {
        let mut cfg = Config {
            replace_builtin_recipes: true,
            ..Default::default()
        };
        cfg.recipes.push(RecipeRule {
            key: 'z',
            name: "only mine".into(),
            detail: String::new(),
            max_risk: RiskName::Safe,
            matches: vec![],
        });

        let recipes = compile(&cfg);
        assert_eq!(recipes.len(), 1);
        assert_eq!(recipes[0].key, 'z');
    }
}
