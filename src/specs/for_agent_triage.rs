//! How reap sorts what a coding agent leaves behind.
//!
//! Two different things live under the same dot-directory, and the whole value
//! of this category is refusing to treat them alike. A tool's cache, its logs
//! and its downloaded packages are ordinary debris, graded by what it takes to
//! get the bytes back. Its session transcripts are not: nothing regenerates a
//! conversation, and the machine holds no other copy of one.
//!
//! What is specified here is therefore mostly the same restraint `Personal`
//! shows, plus the two pieces of evidence this category actually has: whether
//! the project a store belongs to still exists, and whether anything is still
//! writing to it.
//!
//! The second is load-bearing and easy to get wrong. A transcript is appended
//! to for as long as a session lasts, and appending to a file does not touch
//! the directory holding it — so a store's own timestamp can read as months
//! old while a conversation is running inside it. Everything below that talks
//! about age is really specifying that reap looks deeper than that.

use crate::config::RiskName;
use crate::model::{Candidate, Category, Eligibility, Risk};
use crate::specs::given::an_agent_home::an_agent_home;
use std::sync::LazyLock;

fn labels(found: &[Candidate]) -> Vec<String> {
    found.iter().map(|c| c.label.clone()).collect()
}

fn only(found: &[Candidate]) -> &Candidate {
    assert_eq!(
        found.len(),
        1,
        "expected one candidate: {:?}",
        labels(found)
    );
    &found[0]
}

// Resolving a flattened name back to a checkout reads the scheme the unix
// tools write: an absolute path starting at a separator. A Windows path starts
// at a drive instead, and how each tool spells that is something reap would
// have to be shown rather than assume — so there it names the store as stored
// and says it cannot place it. The scenarios that turn on resolution are
// therefore about unix, and say so here rather than by quietly failing in CI.
// Everything that does not depend on it — the freshness floor, the month
// layout, an unreadable name, a configured rule — is specified on every
// platform.
#[cfg(unix)]
mod when_an_agent_holds_sessions_for_a_project_that_still_exists {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        an_agent_home::new()
            .with_sessions_for_a_project_named("payments-api", 4096)
            .candidates()
    });

    #[test]
    fn should_name_the_project_rather_than_the_directory_on_disk() {
        // The directory is called `-tmp-reap-spec-agent-home-3-checkouts-...`,
        // which answers no question anybody has.
        assert_eq!(labels(&BECAUSE), ["an agent · payments-api"]);
    }

    #[test]
    fn should_show_where_that_project_is() {
        // The point of the row is deciding whether you still care about that
        // work, and the path is what tells you which work it was.
        assert!(
            only(&BECAUSE).detail.contains("payments-api"),
            "{}",
            only(&BECAUSE).detail
        );
    }

    #[test]
    fn should_say_how_long_it_has_been_since_anything_was_written() {
        // The evidence, stated on the row that rests on it. Without it the
        // reader is being asked to take "this is finished" on trust.
        assert!(
            only(&BECAUSE).detail.contains("nothing written for"),
            "{}",
            only(&BECAUSE).detail
        );
    }

    #[test]
    fn should_age_it_by_the_transcript_rather_than_by_the_directory() {
        assert_eq!(only(&BECAUSE).age_days, Some(120));
    }

    #[test]
    fn should_grade_the_transcripts_irreversible() {
        // The load-bearing assertion. Irreversible keeps these out of `s`, out
        // of every safe recipe, out of an unattended `--reap` below the
        // irreversible ceiling, and behind a confirmation that has to be typed.
        assert_eq!(only(&BECAUSE).risk, Risk::Danger);
    }
}

#[cfg(unix)]
mod when_no_project_of_that_name_is_on_the_machine_any_more {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        an_agent_home::new()
            .with_sessions_for_a_project_since_deleted("abandoned-spike", 4096)
            .candidates()
    });

    #[test]
    fn should_report_what_it_observed_and_not_what_it_concluded() {
        // The distinction this scenario exists for. reap saw that nothing of
        // that name is there; it did not see a deletion. A checkout on an
        // external disk that is merely unplugged is indistinguishable from one
        // that was removed, and the reader is the one who knows which — so the
        // row states the observation and offers both readings.
        let detail = &only(&BECAUSE).detail;
        assert!(
            detail.contains("no directory of that name is on this machine now"),
            "{detail}"
        );
        assert!(detail.contains("not mounted"), "{detail}");
    }

    #[test]
    fn should_not_treat_a_missing_project_as_permission() {
        // If anything this makes the transcripts more valuable, not less: with
        // the checkout gone they are the last remaining record of that work.
        assert_eq!(only(&BECAUSE).risk, Risk::Danger);
    }

    #[test]
    fn should_still_show_which_store_it_is() {
        assert!(
            only(&BECAUSE).label.contains("abandoned-spike"),
            "{}",
            only(&BECAUSE).label
        );
    }
}

mod when_a_store_is_filed_under_something_that_is_not_a_path {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        an_agent_home::new()
            .with_sessions_filed_under("9f3c1ab2e4", 4096)
            .candidates()
    });

    #[test]
    fn should_offer_it_under_the_name_it_has() {
        // Workspace ids and hashes are shaped like flattened paths and mean
        // nothing like them. Printing a confidently wrong path would be worse
        // than printing the id.
        assert_eq!(labels(&BECAUSE), ["an agent · 9f3c1ab2e4"]);
    }

    #[test]
    fn should_say_it_cannot_place_it_rather_than_that_it_is_missing() {
        // "Nothing of that name exists" is a claim about a path, and this name
        // never was one. Borrowing the wording from the case above would turn
        // an unreadable id into a statement about the disk.
        let detail = &only(&BECAUSE).detail;
        assert!(detail.contains("cannot tell which project"), "{detail}");
        assert!(!detail.contains("no directory of that name"), "{detail}");
    }
}

mod when_a_tool_files_its_sessions_by_date {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        an_agent_home::new()
            .with_a_days_sessions_on("2026/05/27", 1024)
            .with_a_days_sessions_on("2026/05/28", 1024)
            .with_a_days_sessions_on("2026/03/02", 2048)
            .candidates_by_month()
    });

    #[test]
    fn should_offer_a_month_at_a_time() {
        // A row per day is three hundred rows nobody reads; the whole tree as
        // one row is an all-or-nothing decision about years of history, which
        // is the decision people decline to make.
        let mut found = labels(&BECAUSE);
        found.sort();
        assert_eq!(
            found,
            ["an agent sessions · 2026-03", "an agent sessions · 2026-05"]
        );
    }

    #[test]
    fn should_add_up_the_days_inside_the_month() {
        let may = BECAUSE
            .iter()
            .find(|c| c.label.ends_with("2026-05"))
            .expect("the month");
        assert_eq!(may.size, 2048);
    }

    #[test]
    fn should_grade_them_irreversible_too() {
        for candidate in BECAUSE.iter() {
            assert_eq!(
                candidate.risk,
                Risk::Danger,
                "{} was graded {}",
                candidate.label,
                candidate.risk.label()
            );
        }
    }
}

mod when_a_session_is_still_being_written_to {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        an_agent_home::new()
            .with_a_session_still_being_written_for("live-work", 4096)
            .with_sessions_for_a_project_named("finished-work", 4096)
            .candidates()
    });

    #[test]
    fn should_catalogue_the_live_session_as_active_and_not_selectable() {
        let live = BECAUSE
            .iter()
            .find(|candidate| candidate.label.contains("live-work"))
            .expect("the active session remains visible");
        assert_eq!(live.eligibility, Eligibility::Active);
        assert!(!live.selectable());
    }

    #[test]
    #[cfg(unix)]
    fn should_keep_the_finished_one_reclaimable() {
        let finished = BECAUSE
            .iter()
            .find(|candidate| candidate.label.contains("finished-work"))
            .expect("the finished session");
        assert_eq!(finished.eligibility, Eligibility::Reclaimable);
    }
}

mod when_a_session_ended_only_hours_ago {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        an_agent_home::new()
            .with_a_session_last_written_for("this-morning", 3, 4096)
            .candidates()
    });

    #[test]
    fn should_still_not_call_it_finished() {
        // These specifications hold nothing back — `stale_days` is zero here.
        // The floor under a session is not the user's dial to lower.
        let active = only(&BECAUSE);
        assert_eq!(active.eligibility, Eligibility::Active);
        assert!(!active.selectable());
    }
}

mod when_the_configuration_names_an_agent_reap_does_not_ship_knowing_about {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        an_agent_home::new()
            .with_sessions_filed_under("some-conversation", 4096)
            .candidates_for_a_configured_rule(RiskName::Irreversible)
    });

    #[test]
    fn should_offer_it() {
        // The promise the rest of the configuration makes and this category has
        // to keep too: a tool reap has never heard of is reachable without a
        // recompile.
        assert_eq!(labels(&BECAUSE), ["a configured directory"]);
    }

    #[test]
    fn should_file_it_with_the_agents_rather_than_with_the_caches() {
        // Which is the entire reason this is its own table rather than a
        // `[[cache]]` entry: a directory of transcripts listed beside a package
        // cache is a directory of transcripts nobody looks twice at.
        assert_eq!(only(&BECAUSE).category, Category::Agents);
        assert_eq!(only(&BECAUSE).group, "a configured agent");
    }

    #[test]
    fn should_grade_it_the_way_the_rule_asked() {
        assert_eq!(only(&BECAUSE).risk, Risk::Danger);
    }

    #[test]
    fn should_hold_it_to_the_same_proof_of_being_finished() {
        // A configured rule is not a way around the guard. This store was
        // written to long ago, so it is offered and carries the same evidence
        // on the row as a built-in one.
        assert!(
            only(&BECAUSE).detail.contains("put there by a rule"),
            "{}",
            only(&BECAUSE).detail
        );
        assert_eq!(only(&BECAUSE).age_days, Some(120));
    }
}

mod when_a_configured_agent_directory_is_still_in_use {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        an_agent_home::new()
            .with_a_session_still_being_written_for("live-work", 4096)
            .candidates_for_a_configured_rule(RiskName::Safe)
    });

    #[test]
    fn should_be_visible_but_unselectable_even_though_the_rule_calls_it_safe() {
        // The dangerous combination: a rule that says safe, over a directory
        // something is writing to. Eligibility must override that grading.
        let active = only(&BECAUSE);
        assert_eq!(active.eligibility, Eligibility::Active);
        assert!(!active.selectable());
    }
}

mod when_one_day_of_a_month_is_still_being_written_to {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        an_agent_home::new()
            .with_a_days_sessions_on("2026/05/27", 1024)
            .with_a_days_sessions_still_being_written_on("2026/05/28", 1024)
            .with_a_days_sessions_on("2026/03/02", 2048)
            .candidates_by_month()
    });

    #[test]
    fn should_catalogue_the_whole_month_as_active() {
        let may = BECAUSE
            .iter()
            .find(|candidate| candidate.label.contains("2026-05"))
            .expect("the active month remains visible");
        assert_eq!(may.eligibility, Eligibility::Active);
        assert!(!may.selectable());
        let march = BECAUSE
            .iter()
            .find(|candidate| candidate.label.contains("2026-03"))
            .expect("the finished month");
        assert_eq!(march.eligibility, Eligibility::Reclaimable);
    }

    #[test]
    fn should_not_offer_the_quiet_days_of_that_month_separately() {
        // Splitting the month to rescue the 27th would hand back a row whose
        // neighbours are live, at a granularity nobody asked for.
        assert!(
            !labels(&BECAUSE).iter().any(|l| l.contains("27")),
            "{:?}",
            labels(&BECAUSE)
        );
    }
}

mod when_a_store_is_not_laid_out_the_way_reap_expects {
    use super::*;

    static BY_DATE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        an_agent_home::new()
            .with_sessions_filed_under("last-tuesday", 4096)
            .candidates_by_month()
    });

    static PER_PROJECT: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        an_agent_home::new()
            .with_sessions_in_a_loose_file("session-one.jsonl", 4096)
            .candidates()
    });

    #[test]
    fn should_report_a_dated_store_whole_rather_than_hide_it() {
        // Being wrong about the shape of a directory must not make its
        // gigabytes invisible. reap not understanding a layout is a claim about
        // reap; the disk is full either way.
        assert_eq!(only(&BY_DATE).size, 4096);
    }

    #[test]
    fn should_report_a_per_project_store_whole_rather_than_hide_it() {
        // The same failure from the other direction: a tool that files sessions
        // as loose files has no per-project directories to walk, and walking
        // for them and finding none must not come out as an empty disk.
        assert_eq!(only(&PER_PROJECT).size, 4096);
    }

    #[test]
    fn should_admit_that_is_what_it_is_doing() {
        // An all-or-nothing row that does not say it is one reads like a
        // considered decision about a single project.
        for found in [&*BY_DATE, &*PER_PROJECT] {
            let detail = &only(found).detail;
            assert!(
                detail.contains("does not recognise how these are filed"),
                "{detail}"
            );
        }
    }

    #[test]
    fn should_still_grade_it_irreversible() {
        for found in [&*BY_DATE, &*PER_PROJECT] {
            assert_eq!(only(found).risk, Risk::Danger);
        }
    }
}
