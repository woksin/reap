//! What reap offers from a docker daemon, and what it promises those items are
//! worth.
//!
//! Docker is the one scanner whose figures reap does not measure for itself: it
//! repeats what the daemon says. That makes two things worth pinning down —
//! that the right figure is repeated (the space that actually comes back, not
//! the space an image occupies), and that a figure it cannot read is never
//! passed off as zero.

use super::given::a_docker_daemon::a_docker_daemon;
use crate::config::Config;
use crate::model::{Candidate, Risk};
use std::sync::LazyLock;

fn in_group<'a>(candidates: &'a [Candidate], group: &str) -> Vec<&'a Candidate> {
    candidates.iter().filter(|c| c.group == group).collect()
}

fn labels(candidates: &[Candidate]) -> Vec<&str> {
    candidates.iter().map(|c| c.label.as_str()).collect()
}

fn labels_in<'a>(candidates: &'a [Candidate], group: &str) -> Vec<&'a str> {
    candidates
        .iter()
        .filter(|c| c.group == group)
        .map(|c| c.label.as_str())
        .collect()
}

fn named<'a>(candidates: &'a [Candidate], label: &str) -> &'a Candidate {
    candidates
        .iter()
        .find(|c| c.label == label)
        .unwrap_or_else(|| {
            panic!(
                "no candidate named {label}, found: {:?}",
                labels(candidates)
            )
        })
}

mod when_reading_what_a_real_daemon_reported {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> =
        LazyLock::new(|| a_docker_daemon::reporting_what_was_captured().candidates());

    #[test]
    fn should_offer_the_images_no_container_is_using() {
        assert_eq!(
            labels_in(&BECAUSE, "unused images"),
            ["postgres:16", "redis:7-alpine", "node:20-alpine"]
        );
    }

    #[test]
    fn should_leave_the_image_backing_a_live_container_alone() {
        assert!(
            !labels(&BECAUSE).contains(&"nginx:latest"),
            "found: {:?}",
            labels(&BECAUSE)
        );
    }

    #[test]
    fn should_size_an_image_by_what_deleting_it_actually_frees() {
        // The capture says postgres:16 occupies 1.88GB of which only 246.9MB
        // is its own; the rest is layers shared with images that are staying.
        // Reporting the total would promise 1.88 GB and deliver a fifth of it.
        assert_eq!(named(&BECAUSE, "postgres:16").size, 246_900_000);
    }

    #[test]
    fn should_say_how_much_of_the_image_is_shared_with_others() {
        assert_eq!(
            named(&BECAUSE, "postgres:16").detail,
            "no containers · 1.88 GB total, 1.63 GB shared with other images"
        );
    }

    #[test]
    fn should_separate_dangling_images_from_tagged_ones() {
        // An untagged leftover has no purpose left; a tagged image might be
        // wanted again, so the two carry different risk.
        let dangling = in_group(&BECAUSE, "dangling images");
        assert_eq!(dangling.len(), 1);
        assert_eq!(dangling[0].risk, Risk::Safe);
        assert_eq!(named(&BECAUSE, "postgres:16").risk, Risk::Caution);
    }

    #[test]
    fn should_remove_a_tagged_image_by_its_tag_rather_than_its_id() {
        // An image carrying several tags loses only the one named. By ID it
        // would lose all of them.
        assert_eq!(
            named(&BECAUSE, "postgres:16").action.describe(),
            "docker rmi postgres:16"
        );
    }

    #[test]
    fn should_offer_every_container_that_is_not_up() {
        assert_eq!(
            labels_in(&BECAUSE, "stopped containers"),
            ["db-1", "cache-1"]
        );
    }

    #[test]
    fn should_leave_a_running_container_alone() {
        assert!(
            !labels(&BECAUSE).contains(&"web-1"),
            "found: {:?}",
            labels(&BECAUSE)
        );
    }

    #[test]
    fn should_offer_volumes_no_container_holds() {
        assert_eq!(labels_in(&BECAUSE, "unused volumes"), ["project_pgdata"]);
        assert!(
            !labels(&BECAUSE).contains(&"project_in_use"),
            "a volume with a container attached must not be offered"
        );
    }

    #[test]
    fn should_call_every_volume_irreversible() {
        // Volumes are the one place in docker where data nobody can rebuild
        // actually lives — a database's contents, not a rebuildable layer.
        for v in in_group(&BECAUSE, "unused volumes")
            .into_iter()
            .chain(in_group(&BECAUSE, "anonymous volumes"))
        {
            assert_eq!(v.risk, Risk::Danger, "{}", v.label);
        }
    }

    #[test]
    fn should_recognise_an_anonymous_volume_by_the_label_docker_puts_on_it() {
        let anon = in_group(&BECAUSE, "anonymous volumes");
        assert_eq!(anon.len(), 1);
        assert_eq!(anon[0].size, 184_300_000);
    }

    #[test]
    fn should_gather_the_reclaimable_build_cache_into_one_item() {
        // BuildKit records cannot be pruned individually, so offering them
        // separately would offer something that cannot be done.
        let cache = in_group(&BECAUSE, "build cache");
        assert_eq!(cache.len(), 1);
        assert_eq!(
            cache[0].action.describe(),
            "docker builder prune --all --force"
        );
    }

    #[test]
    fn should_count_only_the_records_that_are_neither_in_use_nor_shared() {
        // The capture holds four records: 13.6MB and 1.2GB reclaimable, plus
        // an in-use 892MB and a shared 377.8MB that pruning would not free.
        assert_eq!(in_group(&BECAUSE, "build cache")[0].size, 1_213_600_000);
    }

    #[test]
    fn should_age_the_build_cache_by_its_stalest_record() {
        assert_eq!(in_group(&BECAUSE, "build cache")[0].age_days, Some(21));
    }
}

mod when_docker_states_a_size_in_a_form_reap_cannot_read {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_docker_daemon::reporting_nothing()
            // A shape docker does not emit today. The point is what happens on
            // the day it does.
            .with_an_unused_image("mystery:latest", "12 quorks", "12 quorks")
            .candidates()
    });

    #[test]
    fn should_still_offer_the_item() {
        assert_eq!(labels(&BECAUSE), ["mystery:latest"]);
    }

    #[test]
    fn should_say_the_figure_could_not_be_read() {
        assert!(
            BECAUSE[0].detail.contains("size unrecognised"),
            "detail was: {}",
            BECAUSE[0].detail
        );
    }

    #[test]
    fn should_not_claim_the_item_is_empty() {
        // Zero is a claim about the world — that deleting this frees nothing.
        // Not knowing is a claim about reap, and the detail line makes it.
        assert!(
            !BECAUSE[0].detail.contains("0 B"),
            "detail was: {}",
            BECAUSE[0].detail
        );
    }

    #[test]
    fn should_keep_it_visible_beneath_a_size_floor() {
        // The floor exists to hide items too small to bother with. An
        // unreadable size is not a small one, and hiding it would take the
        // only evidence that anything is wrong off the screen.
        let above = a_docker_daemon::reporting_nothing()
            .with_an_unused_image("mystery:latest", "12 quorks", "12 quorks")
            .candidates_above(100_000_000);
        assert_eq!(labels(&above), ["mystery:latest"]);
    }
}

mod when_a_volume_carries_no_name_of_its_own {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_docker_daemon::reporting_nothing()
            // Docker labels the volumes it named for itself — but only since
            // it started doing so. Older daemons leave nothing behind but the
            // shape of the name: the 64 hex characters of a generated id.
            .with_a_volume(&"a".repeat(64), 0, "40MB")
            .with_a_volume("project_data", 0, "40MB")
            .with_an_anonymous_volume("40MB")
            .candidates()
    });

    #[test]
    fn should_recognise_one_by_its_label() {
        assert_eq!(in_group(&BECAUSE, "anonymous volumes").len(), 2);
    }

    #[test]
    fn should_recognise_one_by_the_shape_of_a_generated_id() {
        assert!(
            labels_in(&BECAUSE, "anonymous volumes")
                .iter()
                .any(|l| l.starts_with("anon  aaaa")),
            "found: {:?}",
            labels_in(&BECAUSE, "anonymous volumes")
        );
    }

    #[test]
    fn should_leave_a_volume_someone_named_in_its_own_group() {
        // A name a person chose is evidence someone meant to keep it.
        assert_eq!(labels_in(&BECAUSE, "unused volumes"), ["project_data"]);
    }
}

mod when_a_stopped_containers_size_cannot_be_read {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_docker_daemon::reporting_nothing()
            .with_a_container("orphan-1", "exited", "12 quorks")
            .candidates()
    });

    #[test]
    fn should_still_offer_it() {
        assert_eq!(labels(&BECAUSE), ["orphan-1"]);
    }

    #[test]
    fn should_say_the_figure_could_not_be_read() {
        assert!(
            BECAUSE[0].detail.contains("size unrecognised"),
            "detail was: {}",
            BECAUSE[0].detail
        );
    }
}

mod when_every_reclaimable_build_cache_record_is_unreadable {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_docker_daemon::reporting_nothing()
            .with_a_build_cache_record("12 quorks", false, false)
            .with_a_build_cache_record("3 quorks", false, false)
            .candidates()
    });

    #[test]
    fn should_still_offer_the_prune() {
        // On most machines this group is the single biggest number on offer,
        // which is exactly why it must not vanish when the total reads zero.
        assert_eq!(in_group(&BECAUSE, "build cache").len(), 1);
    }

    #[test]
    fn should_say_how_many_records_it_could_not_read() {
        assert!(
            BECAUSE[0].detail.contains("2 of them size unrecognised"),
            "detail was: {}",
            BECAUSE[0].detail
        );
    }

    #[test]
    fn should_present_the_total_as_a_floor_rather_than_a_figure() {
        assert!(
            BECAUSE[0].detail.contains("floor"),
            "detail was: {}",
            BECAUSE[0].detail
        );
    }
}

mod when_there_is_no_reclaimable_build_cache {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_docker_daemon::reporting_nothing()
            .with_a_build_cache_record("2.4GB", true, false)
            .with_a_build_cache_record("1.1GB", false, true)
            .candidates()
    });

    #[test]
    fn should_offer_nothing() {
        // Every record is either in use or shared with a build that is not.
        assert!(BECAUSE.is_empty(), "found: {:?}", labels(&BECAUSE));
    }
}

/// Cross-check against the daemon on this machine, rather than against the
/// capture.
///
/// The capture can only ever confirm that reap still reads the output of the
/// docker that produced it. The failure worth catching is the one that arrives
/// later, when a daemon starts spelling a figure differently — so this asks the
/// real one, and is run deliberately:
///
/// ```text
/// cargo test daemon_on_this_machine -- --ignored --nocapture
/// ```
mod when_checked_against_the_daemon_on_this_machine {
    use super::*;
    use crate::scan::docker::{parse_since, parse_size};

    #[test]
    #[ignore = "needs a live docker daemon"]
    fn should_understand_every_figure_it_reports() {
        let Some(df) = live_df() else {
            eprintln!("no docker daemon — nothing to check against");
            return;
        };

        let mut unreadable: Vec<(String, String)> = Vec::new();
        for (kind, records) in df.as_object().into_iter().flatten() {
            for record in records.as_array().into_iter().flatten() {
                for field in ["Size", "UniqueSize", "SharedSize"] {
                    if let Some(s) = record.get(field).and_then(|v| v.as_str())
                        // "N/A" is docker declining to measure, not a figure
                        // in a form reap failed to read.
                        && !matches!(s, "N/A" | "")
                        && parse_size(s).is_none()
                    {
                        unreadable.push((format!("{kind}.{field}"), s.to_string()));
                    }
                }
                for field in ["CreatedSince", "LastUsedSince"] {
                    if let Some(s) = record.get(field).and_then(|v| v.as_str())
                        && !matches!(s, "N/A" | "")
                        && parse_since(s).is_none()
                    {
                        unreadable.push((format!("{kind}.{field}"), s.to_string()));
                    }
                }
            }
        }

        assert!(
            unreadable.is_empty(),
            "docker on this machine states figures reap cannot read: {unreadable:#?}"
        );
    }

    #[test]
    #[ignore = "needs a live docker daemon"]
    fn should_total_what_docker_itself_calls_reclaimable() {
        // Not an assertion: `docker system df` totals whole images, while reap
        // counts only the unique bytes of the ones no container is using, so
        // the two legitimately differ. Printed side by side, the figures are
        // checkable by eye against the tool everyone already trusts.
        let Some(df) = live_df() else {
            eprintln!("no docker daemon — nothing to check against");
            return;
        };
        let found = a_docker_daemon::reporting_exactly(df).candidates();
        for group in [
            "unused images",
            "dangling images",
            "stopped containers",
            "unused volumes",
            "anonymous volumes",
            "build cache",
        ] {
            let items = in_group(&found, group);
            let total: u64 = items.iter().map(|c| c.size).sum();
            println!(
                "{group:>20}  {:>4} items  {:>10}",
                items.len(),
                crate::util::human(total)
            );
        }
        println!("\ncompare against:  docker system df");
        let _ = std::process::Command::new("docker")
            .args(["system", "df"])
            .status();
    }

    fn live_df() -> Option<serde_json::Value> {
        let out = std::process::Command::new("docker")
            .args(["system", "df", "-v", "--format", "{{json .}}"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        serde_json::from_slice(&out.stdout).ok()
    }
}

mod when_the_configuration_ignores_a_docker_group {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        let mut cfg = Config::default();
        // The pattern pressing `x` on a docker item writes, and the one the
        // README documents. Docker items have no path, so the group is the
        // only handle there is.
        cfg.ignore.push("docker/unused volumes".into());
        a_docker_daemon::reporting_what_was_captured().candidates_with(&cfg)
    });

    #[test]
    fn should_withhold_that_group() {
        assert!(
            in_group(&BECAUSE, "unused volumes").is_empty(),
            "found: {:?}",
            labels(&BECAUSE)
        );
    }

    #[test]
    fn should_still_offer_the_other_groups() {
        assert!(!in_group(&BECAUSE, "anonymous volumes").is_empty());
        assert!(!in_group(&BECAUSE, "unused images").is_empty());
    }
}
