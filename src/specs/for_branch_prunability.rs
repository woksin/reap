//! What reap concludes about a branch, and therefore how dangerous it says
//! deleting one would be.
//!
//! This is the judgement the whole tool rests on. A branch that looks identical
//! to `git branch --merged` may hold the only copy of a week's work, or none at
//! all, and the difference is what these specifications pin down.

use super::given::a_repository::a_repository;
use crate::model::{Candidate, Risk};
use std::sync::LazyLock;

/// The single candidate for `name`, or a failure naming what was found instead.
fn the_branch<'a>(candidates: &'a [Candidate], name: &str) -> &'a Candidate {
    let suffix = format!("/{name}");
    candidates
        .iter()
        .find(|c| c.label.ends_with(&suffix))
        .unwrap_or_else(|| {
            panic!(
                "no candidate for branch {name}; found: {:?}",
                candidates.iter().map(|c| &c.label).collect::<Vec<_>>()
            )
        })
}

fn deletes_with(candidate: &Candidate) -> String {
    candidate.action.describe()
}

mod when_a_branch_has_been_merged_into_main {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_repository::new()
            .with_a_branch_merged_into_main("feature/done")
            .candidates()
    });

    #[test]
    fn should_group_it_as_merged() {
        assert_eq!(
            the_branch(&BECAUSE, "feature/done").group,
            "merged branches"
        );
    }

    #[test]
    fn should_consider_it_safe_to_delete() {
        assert_eq!(the_branch(&BECAUSE, "feature/done").risk, Risk::Safe);
    }

    #[test]
    fn should_say_it_is_already_merged() {
        assert!(
            the_branch(&BECAUSE, "feature/done")
                .detail
                .contains("already merged"),
            "detail was: {}",
            the_branch(&BECAUSE, "feature/done").detail
        );
    }

    #[test]
    fn should_not_need_to_force_the_delete() {
        // `-d` refuses to discard unmerged work; reaching for `-D` here would
        // throw away git's own last line of defence.
        assert!(
            deletes_with(the_branch(&BECAUSE, "feature/done")).contains(" -d "),
            "expected a safe delete, got: {}",
            deletes_with(the_branch(&BECAUSE, "feature/done"))
        );
    }
}

mod when_a_branch_was_squash_merged {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_repository::new()
            .with_a_branch_squash_merged_into_main("feature/squashed")
            .candidates()
    });

    #[test]
    fn should_recognise_the_work_is_already_upstream() {
        // `--merged` calls this branch unmerged; only comparing patch ids
        // reveals that main already contains every line of it.
        assert_eq!(
            the_branch(&BECAUSE, "feature/squashed").group,
            "squash-merged branches"
        );
    }

    #[test]
    fn should_consider_it_safe_to_delete() {
        assert_eq!(the_branch(&BECAUSE, "feature/squashed").risk, Risk::Safe);
    }

    #[test]
    fn should_explain_that_the_content_is_upstream() {
        assert!(
            the_branch(&BECAUSE, "feature/squashed")
                .detail
                .contains("by content"),
            "detail was: {}",
            the_branch(&BECAUSE, "feature/squashed").detail
        );
    }

    #[test]
    fn should_force_the_delete_since_git_still_calls_it_unmerged() {
        assert!(
            deletes_with(the_branch(&BECAUSE, "feature/squashed")).contains(" -D "),
            "expected a forced delete, got: {}",
            deletes_with(the_branch(&BECAUSE, "feature/squashed"))
        );
    }
}

mod when_a_branch_is_unmerged_but_pushed {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_repository::new()
            .with_a_branch_pushed_but_unmerged("feature/in-review")
            .candidates()
    });

    #[test]
    fn should_group_it_as_pushed() {
        assert_eq!(
            the_branch(&BECAUSE, "feature/in-review").group,
            "pushed branches"
        );
    }

    #[test]
    fn should_consider_it_rebuildable_rather_than_irreversible() {
        // The commits survive on the remote, so deleting locally costs a fetch,
        // not the work.
        assert_eq!(
            the_branch(&BECAUSE, "feature/in-review").risk,
            Risk::Caution
        );
    }

    #[test]
    fn should_say_it_is_recoverable_from_the_remote() {
        assert!(
            the_branch(&BECAUSE, "feature/in-review")
                .detail
                .contains("recoverable from the remote"),
            "detail was: {}",
            the_branch(&BECAUSE, "feature/in-review").detail
        );
    }
}

mod when_a_branch_was_never_pushed {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_repository::new()
            .with_a_branch_never_pushed("feature/local-only")
            .candidates()
    });

    #[test]
    fn should_group_it_as_unpushed() {
        assert_eq!(
            the_branch(&BECAUSE, "feature/local-only").group,
            "unpushed branches"
        );
    }

    #[test]
    fn should_consider_it_irreversible() {
        assert_eq!(
            the_branch(&BECAUSE, "feature/local-only").risk,
            Risk::Danger
        );
    }

    #[test]
    fn should_say_the_commits_exist_nowhere_else() {
        assert!(
            the_branch(&BECAUSE, "feature/local-only")
                .detail
                .contains("exist only here"),
            "detail was: {}",
            the_branch(&BECAUSE, "feature/local-only").detail
        );
    }

    #[test]
    fn should_count_the_commits_at_stake() {
        assert!(
            the_branch(&BECAUSE, "feature/local-only")
                .detail
                .starts_with('1'),
            "expected the commit count first, got: {}",
            the_branch(&BECAUSE, "feature/local-only").detail
        );
    }
}

mod when_a_branchs_upstream_was_deleted_while_it_held_local_commits {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_repository::new()
            .with_a_branch_whose_upstream_was_deleted("feature/orphaned")
            .candidates()
    });

    #[test]
    fn should_treat_it_as_irreversible_rather_than_assume_a_squash_merge() {
        // A deleted upstream usually means a merged pull request — but not
        // always, and guessing wrong here destroys work.
        assert_eq!(the_branch(&BECAUSE, "feature/orphaned").risk, Risk::Danger);
    }

    #[test]
    fn should_say_the_upstream_was_deleted_rather_than_that_it_lacks_the_commits() {
        // The upstream no longer exists, so describing it as merely missing
        // them would misdescribe what happened.
        assert!(
            the_branch(&BECAUSE, "feature/orphaned")
                .detail
                .contains("was deleted"),
            "detail was: {}",
            the_branch(&BECAUSE, "feature/orphaned").detail
        );
    }
}

mod when_a_repository_has_branches_of_every_kind {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_repository::new()
            .with_a_branch_merged_into_main("merged")
            .with_a_branch_squash_merged_into_main("squashed")
            .with_a_branch_pushed_but_unmerged("pushed")
            .with_a_branch_never_pushed("local")
            .candidates()
    });

    #[test]
    fn should_separate_them_into_their_own_groups() {
        for (branch, group) in [
            ("merged", "merged branches"),
            ("squashed", "squash-merged branches"),
            ("pushed", "pushed branches"),
            ("local", "unpushed branches"),
        ] {
            assert_eq!(
                the_branch(&BECAUSE, branch).group,
                group,
                "branch {branch} landed in the wrong group"
            );
        }
    }

    #[test]
    fn should_leave_only_the_local_one_irreversible() {
        let irreversible: Vec<&str> = BECAUSE
            .iter()
            .filter(|c| c.risk == Risk::Danger && c.group.contains("branches"))
            .map(|c| c.label.as_str())
            .collect();
        assert_eq!(irreversible.len(), 1, "got: {irreversible:?}");
        assert!(irreversible[0].ends_with("/local"));
    }

    #[test]
    fn should_never_offer_the_branch_that_is_checked_out() {
        assert!(
            !BECAUSE.iter().any(|c| c.label.ends_with("/main")),
            "the current branch must not be offered for deletion"
        );
    }
}
