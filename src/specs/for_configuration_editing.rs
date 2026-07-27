//! What the settings screen writes, and whether the rest of reap obeys it.
//!
//! The screen is only worth having if a change made on it reaches the scanners,
//! and survives being written to a file and read back. Those are two different
//! failures with the same symptom — the setting appeared to take and did not —
//! so both are specified here rather than assumed from the fact that a field
//! changed in memory.
//!
//! Every scenario therefore ends at one of two places: a candidate the scanner
//! either emits or does not, or a `Config` that has been through TOML.

use crate::config::{Config, RiskName};
use crate::model::{Action, Candidate, Category, Risk};
use crate::settings::{Origin, Row, Settings};
use std::sync::LazyLock;

/// A settings screen with every section open, so a row can be found by hand.
fn opened(cfg: &Config) -> Settings {
    let mut settings = Settings::new(cfg);
    settings.expanded = crate::settings::Section::ALL.into_iter().collect();
    settings.rebuild(cfg);
    settings
}

/// Put the cursor on the built-in cache rule with this label.
fn on_builtin_cache(settings: &mut Settings, cfg: &Config, label: &str) {
    let at = settings
        .rows
        .iter()
        .position(|row| match row {
            Row::Cache(Origin::Builtin, i) => settings
                .cache_rule(cfg, Origin::Builtin, *i)
                .is_some_and(|r| r.label == label),
            _ => false,
        })
        .unwrap_or_else(|| panic!("reap ships with no cache rule called {label}"));
    settings.cursor = at;
}

/// What the scanners would do with this config, for one candidate.
///
/// Goes through `emit`, which is where ignoring and re-grading are actually
/// applied — so this observes the rule reaching the same code a real scan does.
fn offered(cfg: &Config, candidate: Candidate) -> Option<Candidate> {
    let opts = crate::scan::ScanOpts {
        rules: std::sync::Arc::new(crate::scan::Rules::from_config(cfg)),
        ..super::given::scanning_everything(vec![])
    };
    let (tx, rx) = std::sync::mpsc::channel();
    crate::scan::emit(&tx, &opts, candidate);
    drop(tx);
    rx.into_iter().find_map(|e| match e {
        crate::model::ScanEvent::Found(c) => Some(*c),
        _ => None,
    })
}

/// A candidate standing in for what the Firefox cache rule would produce.
fn a_firefox_cache_candidate() -> Candidate {
    Candidate::new(
        Category::Caches,
        "web browsers",
        "Firefox cache",
        "",
        400_000_000,
        Risk::Safe,
        Action::Remove(crate::config::expand("~/Library/Caches/Firefox")),
    )
}

/// A config saved and read back, so nothing survives only in memory.
fn round_tripped(cfg: &Config) -> Config {
    let text = toml::to_string_pretty(cfg).expect("a config reap wrote must serialise");
    toml::from_str(&text).expect("a config reap wrote must parse")
}

mod when_a_built_in_rule_is_turned_off {
    use super::*;

    static BECAUSE: LazyLock<Config> = LazyLock::new(|| {
        let mut cfg = Config::default();
        let mut settings = opened(&cfg);
        on_builtin_cache(&mut settings, &cfg, "Firefox cache");
        settings.toggle_off(&mut cfg);
        cfg
    });

    #[test]
    fn should_write_it_down_as_an_ignore() {
        // The same vocabulary `x` and a hand-written config already use, so
        // nothing learned on one surface stops being true on the other.
        assert!(
            BECAUSE
                .ignore
                .contains(&"~/Library/Caches/Firefox".to_string()),
            "wrote {:?}",
            BECAUSE.ignore
        );
    }

    #[test]
    fn should_stop_the_scanner_offering_it() {
        // The assertion that makes the screen worth having.
        assert!(offered(&BECAUSE, a_firefox_cache_candidate()).is_none());
    }

    #[test]
    fn should_survive_being_written_to_a_file_and_read_back() {
        assert!(offered(&round_tripped(&BECAUSE), a_firefox_cache_candidate()).is_none());
    }

    #[test]
    fn should_leave_every_other_rule_alone() {
        let untouched = Candidate::new(
            Category::Caches,
            "web browsers",
            "Chrome cache",
            "",
            1,
            Risk::Safe,
            Action::Remove(crate::config::expand("~/Library/Caches/Google/Chrome")),
        );
        assert!(offered(&BECAUSE, untouched).is_some());
    }
}

mod when_a_rule_that_was_turned_off_is_turned_back_on {
    use super::*;

    static BECAUSE: LazyLock<Config> = LazyLock::new(|| {
        let mut cfg = Config::default();
        let mut settings = opened(&cfg);
        on_builtin_cache(&mut settings, &cfg, "Firefox cache");
        settings.toggle_off(&mut cfg);
        settings.toggle_off(&mut cfg);
        cfg
    });

    #[test]
    fn should_leave_nothing_behind_in_the_config() {
        // `x` used to be a one-way door — it wrote a line to a file nobody was
        // looking at. Taking it back has to actually remove the line, or the
        // next reader is left interpreting a rule that no longer applies.
        assert!(BECAUSE.ignore.is_empty(), "left {:?}", BECAUSE.ignore);
    }

    #[test]
    fn should_offer_it_again() {
        assert!(offered(&BECAUSE, a_firefox_cache_candidate()).is_some());
    }
}

mod when_a_rule_is_re_graded {
    use super::*;

    static BECAUSE: LazyLock<Config> = LazyLock::new(|| {
        let mut cfg = Config::default();
        let mut settings = opened(&cfg);
        on_builtin_cache(&mut settings, &cfg, "Firefox cache");
        // safe → rebuildable → irreversible.
        settings.cycle_grade(&mut cfg);
        settings.cycle_grade(&mut cfg);
        settings.cycle_grade(&mut cfg);
        cfg
    });

    #[test]
    fn should_write_it_down_as_an_override() {
        assert_eq!(BECAUSE.overrides.len(), 1);
        assert_eq!(BECAUSE.overrides[0].risk, RiskName::Irreversible);
    }

    #[test]
    fn should_change_what_the_scanner_says_it_costs() {
        // Risk is what `s` and every recipe select by, so this is the setting
        // that decides whether one keystroke takes the thing or leaves it.
        let graded = offered(&BECAUSE, a_firefox_cache_candidate()).expect("still offered");
        assert_eq!(graded.risk, Risk::Danger);
    }

    #[test]
    fn should_survive_being_written_to_a_file_and_read_back() {
        let reloaded = offered(&round_tripped(&BECAUSE), a_firefox_cache_candidate());
        assert_eq!(reloaded.expect("still offered").risk, Risk::Danger);
    }
}

mod when_a_rule_is_graded_all_the_way_round {
    use super::*;

    static BECAUSE: LazyLock<Config> = LazyLock::new(|| {
        let mut cfg = Config::default();
        let mut settings = opened(&cfg);
        on_builtin_cache(&mut settings, &cfg, "Firefox cache");
        for _ in 0..4 {
            settings.cycle_grade(&mut cfg);
        }
        cfg
    });

    #[test]
    fn should_go_back_to_the_rules_own_grading_rather_than_pinning_it() {
        // A rule put back must be indistinguishable from one never touched, so
        // that a corrected default in a later release still reaches it.
        assert!(BECAUSE.overrides.is_empty(), "left {:?}", BECAUSE.overrides);
        let restored = offered(&BECAUSE, a_firefox_cache_candidate()).expect("still offered");
        assert_eq!(restored.risk, Risk::Safe);
    }
}

mod when_a_cache_path_is_added {
    use super::*;

    static BECAUSE: LazyLock<Config> = LazyLock::new(|| {
        let mut cfg = Config::default();
        let mut settings = opened(&cfg);
        settings.cursor = settings
            .rows
            .iter()
            .position(|r| *r == Row::Add(crate::settings::Section::Caches))
            .expect("caches take additions");
        settings.begin_add();
        settings.edit.as_mut().unwrap().buffer = "~/.cache/my-tool".into();
        settings.commit_edit(&mut cfg).expect("a plain path");
        cfg
    });

    #[test]
    fn should_name_it_after_the_last_part_of_the_path() {
        // One line of typing per question. The name is a guess reap can make,
        // and `n` corrects it — asking for both up front would be a form.
        assert_eq!(BECAUSE.caches.len(), 1);
        assert_eq!(BECAUSE.caches[0].label, "my-tool");
        assert_eq!(BECAUSE.caches[0].path, "~/.cache/my-tool");
    }

    #[test]
    fn should_start_at_the_cautious_grading() {
        // Something nobody has said anything about yet must not be swept up by
        // a recipe that takes everything safe.
        assert_eq!(BECAUSE.caches[0].risk, RiskName::Rebuildable);
    }

    #[test]
    fn should_reach_the_scanner_as_a_rule() {
        let rules = crate::scan::Rules::from_config(&round_tripped(&BECAUSE));
        assert!(
            rules.caches.iter().any(|r| r.path == "~/.cache/my-tool"),
            "the new rule never reached the scanner"
        );
        // And the built-ins are still there beside it.
        assert!(rules.caches.iter().any(|r| r.label == "npm cache"));
    }
}

mod when_something_is_typed_that_cannot_be_used {
    use super::*;

    #[test]
    fn should_say_why_and_keep_what_was_typed() {
        // Throwing the text away would make a typo cost the whole line.
        let mut cfg = Config::default();
        let mut settings = opened(&cfg);
        settings.cursor = settings
            .rows
            .iter()
            .position(|r| *r == Row::Add(crate::settings::Section::Artifacts))
            .expect("artifacts take additions");
        settings.begin_add();
        settings.edit.as_mut().unwrap().buffer = "src/generated".into();

        let refused = settings.commit_edit(&mut cfg).unwrap_err();
        assert!(refused.contains("directory name"), "{refused}");
        assert_eq!(
            settings.edit.as_ref().map(|e| e.buffer.as_str()),
            Some("src/generated"),
            "the text must still be there to correct"
        );
        assert!(cfg.artifacts.is_empty(), "nothing should have been written");
    }

    #[test]
    fn should_refuse_a_threshold_that_is_not_a_size() {
        let mut cfg = Config::default();
        let mut settings = opened(&cfg);
        settings.cursor = settings
            .rows
            .iter()
            .position(|r| *r == Row::Setting(crate::settings::Setting::MinSize))
            .expect("min size is on the screen");
        settings.begin_edit(&cfg);
        settings.edit.as_mut().unwrap().buffer = "quite big".into();

        let refused = settings.commit_edit(&mut cfg).unwrap_err();
        assert!(refused.contains("number"), "{refused}");
        assert_eq!(cfg.scan.min_size, None, "nothing should have been written");
    }
}

mod when_a_switch_is_turned_off {
    use super::*;

    #[test]
    fn should_stop_that_scanner_running() {
        let mut cfg = Config::default();
        let mut settings = opened(&cfg);
        settings.cursor = settings
            .rows
            .iter()
            .position(|r| *r == Row::Setting(crate::settings::Setting::Personal))
            .expect("the personal switch is on the screen");

        settings.toggle_switch(&mut cfg);
        assert_eq!(cfg.scan.personal, Some(false));
        // Read back the way `main` reads it, so the switch reaches a real run.
        assert_eq!(round_tripped(&cfg).scan.personal, Some(false));
    }
}

mod when_a_built_in_rule_is_deleted {
    use super::*;

    #[test]
    fn should_refuse_and_say_what_to_do_instead() {
        // Deleting a built-in would mean the config replaced reap's defaults
        // rather than adjusting them, and the next release moving a vendor's
        // cache directory would silently stop applying.
        let mut cfg = Config::default();
        let mut settings = opened(&cfg);
        on_builtin_cache(&mut settings, &cfg, "Firefox cache");

        let said = settings.delete(&mut cfg).expect("it should say something");
        assert!(said.contains("turned off with x"), "{said}");
        assert!(cfg.ignore.is_empty(), "nothing should have been written");
    }
}
