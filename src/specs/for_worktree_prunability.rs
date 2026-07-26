//! What reap concludes about a linked worktree.
//!
//! Removing one deletes its working directory outright, so both kinds of loss
//! matter: files never committed, and commits no remote can reach.

use super::given::a_repository::a_repository;
use crate::model::{Candidate, Risk};
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
}

mod when_a_worktree_holds_commits_no_remote_has {
    use super::*;

    static BECAUSE: LazyLock<Vec<Candidate>> = LazyLock::new(|| {
        a_repository::new()
            .with_a_worktree_holding_unpushed_work("wip-checkout")
            .candidates()
    });

    #[test]
    fn should_consider_it_irreversible() {
        // Clean on disk, but the commits exist nowhere else.
        assert_eq!(the_worktree(&BECAUSE, "wip-checkout").risk, Risk::Danger);
    }

    #[test]
    fn should_say_the_commits_exist_only_there() {
        assert!(
            the_worktree(&BECAUSE, "wip-checkout")
                .detail
                .contains("exist only here"),
            "detail was: {}",
            the_worktree(&BECAUSE, "wip-checkout").detail
        );
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
            .find(|c| c.group == "old stashes")
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
    fn should_drop_it_by_name_rather_than_position() {
        let action = the_stash().action.describe();
        assert!(action.contains("stash@{"), "action: {action}");
    }
}
