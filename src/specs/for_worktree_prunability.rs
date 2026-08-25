//! What reap concludes about a linked worktree.
//!
//! Removing one deletes its working directory outright, so both kinds of loss
//! matter: files never committed, and commits no remote can reach.

use super::given::a_repository::a_repository;
use crate::model::{Candidate, Eligibility, Risk};
use std::sync::LazyLock;

fn the_worktree<'a>(candidates: &'a [Candidate], name: &str) -> &'a Candidate {
    candidates
        .iter()
        .filter(|c| c.group.contains("worktree"))
        .find(|c| c.label.contains(name))
        .unwrap_or_else(|| {
            panic!(
                "no worktree candidate for {name}; found: {:?}",
                candidates
                    .iter()
                    .map(|c| (&c.group, &c.label))
                    .collect::<Vec<_>>()
            )
        })
}

mod when_a_worktree_is_clean_and_fully_pushed {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_repository::new()
            .with_a_worktree("spare-checkout")
            .candidates()
    });

    #[test]
    fn should_consider_it_rebuildable_rather_than_irreversible() {
        assert_eq!(the_worktree(&BECAUSE, "spare-checkout").risk, Risk::Caution);
    }

    #[test]
    fn should_say_it_is_safe_to_prune() {
        assert!(
            the_worktree(&BECAUSE, "spare-checkout")
                .detail
                .contains("safe to prune"),
            "detail was: {}",
            the_worktree(&BECAUSE, "spare-checkout").detail
        );
    }

    #[test]
    fn should_remove_it_through_git_rather_than_deleting_the_directory() {
        // Deleting the directory would leave the repository's worktree
        // administration pointing at somewhere that no longer exists.
        let action = the_worktree(&BECAUSE, "spare-checkout").action.describe();
        assert!(
            action.starts_with("git worktree remove"),
            "action: {action}"
        );
    }

    #[test]
    fn should_leave_gits_own_cleanliness_check_in_force() {
        let action = the_worktree(&BECAUSE, "spare-checkout").action.describe();
        assert!(!action.contains("--force"), "action: {action}");
    }
}

mod when_an_attached_worktree_holds_commits_no_remote_has {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_repository::new()
            .with_a_worktree_holding_unpushed_work("wip-checkout")
            .candidates()
    });

    #[test]
    fn should_consider_the_checkout_rebuildable() {
        // `git worktree remove` leaves the attached branch and every commit it
        // names in the shared repository. Not pushed is not lost here.
        assert_eq!(the_worktree(&BECAUSE, "wip-checkout").risk, Risk::Caution);
    }

    #[test]
    fn should_say_the_commits_remain_reachable() {
        assert!(
            the_worktree(&BECAUSE, "wip-checkout")
                .detail
                .contains("remain reachable"),
            "detail was: {}",
            the_worktree(&BECAUSE, "wip-checkout").detail
        );
    }
}

mod when_a_detached_worktree_has_a_commit_no_ref_names {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_repository::new()
            .with_a_detached_worktree_holding_unique_commit("detached-checkout")
            .candidates()
    });

    #[test]
    fn should_consider_removing_it_irreversible() {
        assert_eq!(
            the_worktree(&BECAUSE, "detached-checkout").risk,
            Risk::Danger
        );
    }

    #[test]
    fn should_say_no_surviving_ref_names_the_commit() {
        assert!(
            the_worktree(&BECAUSE, "detached-checkout")
                .detail
                .contains("no surviving ref")
        );
    }
}

mod when_an_agent_worktree_directory_is_no_longer_registered {
    use super::*;
    use crate::model::Eligibility;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_repository::new()
            .with_an_orphaned_agent_worktree("orphan-checkout")
            .candidates()
    });

    #[test]
    fn should_catalogue_it_without_offering_a_blind_recursive_delete() {
        let orphan = the_worktree(&BECAUSE, "orphan-checkout");
        assert_eq!(orphan.eligibility, Eligibility::Protected);
        assert!(!orphan.selectable());
        assert!(orphan.detail.contains("inspect manually"));
    }
}

mod when_a_worktree_has_uncommitted_changes {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_repository::new()
            .with_a_dirty_worktree("dirty-checkout")
            .candidates()
    });

    #[test]
    fn should_consider_it_irreversible() {
        assert_eq!(the_worktree(&BECAUSE, "dirty-checkout").risk, Risk::Danger);
    }

    #[test]
    fn should_report_how_many_files_would_be_lost() {
        assert!(
            the_worktree(&BECAUSE, "dirty-checkout")
                .detail
                .contains("1 uncommitted file"),
            "detail was: {}",
            the_worktree(&BECAUSE, "dirty-checkout").detail
        );
    }
}

mod when_a_worktree_has_an_ignored_file {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_repository::new()
            .with_a_worktree_holding_an_ignored_file("ignored-checkout")
            .candidates()
    });

    #[test]
    fn should_consider_the_ignored_file_irreversible() {
        assert_eq!(
            the_worktree(&BECAUSE, "ignored-checkout").risk,
            Risk::Danger
        );
    }

    #[test]
    fn should_say_what_git_normally_hides() {
        assert!(
            the_worktree(&BECAUSE, "ignored-checkout")
                .detail
                .contains("ignored"),
            "detail was: {}",
            the_worktree(&BECAUSE, "ignored-checkout").detail
        );
    }
}

mod when_a_repository_has_a_stash {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_repository::new()
            .with_a_stash("half-finished")
            .candidates()
    });

    fn the_stash() -> &'static Candidate {
        BECAUSE
            .iter()
            .find(|c| c.group == "protected stashes")
            .expect("a stash candidate")
    }

    #[test]
    fn should_consider_it_irreversible() {
        // A stash is by definition work that exists nowhere else.
        assert_eq!(the_stash().risk, Risk::Danger);
    }

    #[test]
    fn should_carry_the_message_so_it_can_be_recognised() {
        assert!(
            the_stash().detail.contains("half-finished"),
            "detail was: {}",
            the_stash().detail
        );
    }

    #[test]
    fn should_refuse_automated_positional_deletion() {
        assert_eq!(the_stash().eligibility, Eligibility::Protected);
        assert!(!the_stash().selectable());
        assert!(the_stash().detail.contains("drop manually"));
    }
}
