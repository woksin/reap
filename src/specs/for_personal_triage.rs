//! How reap sorts a person's own files.
//!
//! Every other scanner works from proof. A `target` directory beside a
//! `Cargo.toml` is build output; a branch whose commits are all in `main` is
//! merged. There is no equivalent here. A 4 GB file in Downloads is either an
//! installer for something already installed or the only copy of a wedding
//! video, and the filesystem holds nothing that tells them apart.
//!
//! So what is specified here is mostly restraint: which of the two sides an
//! unproven thing is put on, and what it takes to be moved off it. Getting this
//! wrong in the safe direction wastes disk. Getting it wrong in the other
//! direction destroys something that existed in one place.

use crate::model::{Candidate, Eligibility, Risk};
use crate::specs::given::a_download_directory::{a_backup_directory, a_download_directory};
use std::sync::LazyLock;

fn labels(found: &[Candidate]) -> Vec<String> {
    found.iter().map(|c| c.label.clone()).collect()
}

fn find<'a>(found: &'a [Candidate], label: &str) -> &'a Candidate {
    found
        .iter()
        .find(|c| c.label == label)
        .unwrap_or_else(|| panic!("no candidate named {label}, found: {:?}", labels(found)))
}

mod when_an_installer_has_been_sitting_in_downloads {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_download_directory::new()
            .with_a_file("Xcode_15.dmg", 4096, 200)
            .candidates()
    });

    #[test]
    fn should_offer_it() {
        assert_eq!(labels(&BECAUSE), ["Xcode_15.dmg"]);
    }

    #[test]
    fn should_not_assume_another_copy_can_be_downloaded() {
        // A suffix cannot distinguish a public installer from a custom disk
        // image or a locally built package that exists only here.
        assert_eq!(find(&BECAUSE, "Xcode_15.dmg").risk, Risk::Danger);
    }

    #[test]
    fn should_say_why_it_is_being_offered() {
        // The detail line is the only place a non-developer is told what reap
        // thinks this is, and therefore whether reap has it right.
        assert!(
            find(&BECAUSE, "Xcode_15.dmg")
                .detail
                .contains("cannot prove"),
            "{}",
            find(&BECAUSE, "Xcode_15.dmg").detail
        );
    }
}

mod when_a_download_might_be_the_only_copy_of_something {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_download_directory::new()
            .with_a_file("wedding.mov", 4096, 200)
            .with_a_directory("photos-from-mum", 4096, 200)
            .candidates()
    });

    #[test]
    fn should_still_show_it_because_that_is_the_whole_point() {
        // Hiding it would leave the biggest thing on the disk invisible, which
        // is the reason the disk never gets cleaned.
        let mut found = labels(&BECAUSE);
        found.sort();
        assert_eq!(found, ["photos-from-mum", "wedding.mov"]);
    }

    #[test]
    fn should_grade_every_one_of_them_irreversible() {
        // This is the load-bearing assertion in this file. Irreversible is what
        // keeps these out of `s`, out of every safe recipe, out of an
        // unattended `--reap`, and behind a confirmation that has to be typed.
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

    #[test]
    fn should_admit_that_it_does_not_know() {
        assert!(
            find(&BECAUSE, "wedding.mov").detail.contains("cannot tell"),
            "{}",
            find(&BECAUSE, "wedding.mov").detail
        );
    }
}

mod when_a_download_is_recent {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_download_directory::new()
            .where_stale_means(30)
            .with_a_file("just-downloaded.dmg", 4096, 2)
            .with_a_file("forgotten.dmg", 4096, 90)
            .candidates()
    });

    #[test]
    fn should_show_it_as_recent_without_making_it_selectable() {
        // Something downloaded on Tuesday still explains occupied bytes on
        // Wednesday, but it is not a deletion candidate.
        let mut found = labels(&BECAUSE);
        found.sort();
        assert_eq!(found, ["forgotten.dmg", "just-downloaded.dmg"]);
        let recent = find(&BECAUSE, "just-downloaded.dmg");
        assert_eq!(recent.eligibility, Eligibility::Recent);
        assert!(!recent.selectable());
    }
}

mod when_a_download_is_too_small_to_be_worth_a_decision {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_download_directory::new()
            .ignoring_anything_under("1MB")
            .with_a_file("receipt.pdf", 2048, 200)
            .with_a_file("holiday.mp4", 4_000_000, 200)
            .candidates()
    });

    #[test]
    fn should_not_ask_about_it() {
        // Every entry here costs the user a judgement they have to make one at
        // a time, so a list long enough to skim past is a list that gets
        // skipped whole.
        assert_eq!(labels(&BECAUSE), ["holiday.mp4"]);
    }
}

mod when_a_phone_has_been_backed_up_to_this_machine {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_backup_directory::new()
            .with_a_backup_of("Sara's iPhone", 4096)
            .candidates()
    });

    #[test]
    fn should_name_it_after_the_device_rather_than_its_identifier() {
        // The directory is called `00008030-001C4D...`, which tells nobody
        // whether this is the phone they still have.
        assert_eq!(labels(&BECAUSE), ["Sara's iPhone"]);
    }

    #[test]
    fn should_grade_it_irreversible() {
        // Frequently the largest single thing on a machine, and frequently the
        // only remaining copy of a phone that no longer exists.
        assert_eq!(BECAUSE[0].risk, Risk::Danger);
    }
}
