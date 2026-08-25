//! Unique-byte accounting for overlapping findings.
//!
//! A worktree can contain a `node_modules`, `bin`, and `obj`, all of which are
//! useful findings in their own right. Summing every row promises the same
//! bytes several times. This module treats filesystem footprints as nested
//! ownership: the most specific rows own their bytes and a parent owns only
//! the remainder. For an actual selection the rule reverses naturally — a
//! selected parent removes its whole tree and covers selected descendants.

use crate::model::Candidate;

/// Per-row bytes with nested filesystem findings assigned to the most specific
/// rows. Non-filesystem resources (Docker objects, branches, and so on) retain
/// their reported size.
pub fn exclusive_sizes(items: &[Candidate]) -> Vec<u64> {
    let mut out: Vec<u64> = items.iter().map(|item| item.size).collect();
    let mut paths: Vec<_> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| item.footprint.as_deref().map(|path| (index, path)))
        .collect();
    // Parents sort before descendants, while the original index keeps the
    // first of duplicate footprints as their owner. A stack then finds each
    // row's nearest parent in one pass instead of comparing every pair.
    paths.sort_unstable_by(|(left_index, left_path), (right_index, right_path)| {
        left_path.cmp(right_path).then(left_index.cmp(right_index))
    });

    let mut ancestors: Vec<(usize, &std::path::Path)> = Vec::new();
    for (child_index, child_path) in paths {
        if ancestors
            .last()
            .is_some_and(|(_, owner_path)| *owner_path == child_path)
        {
            out[child_index] = 0;
            continue;
        }
        while ancestors
            .last()
            .is_some_and(|(_, parent_path)| !child_path.starts_with(parent_path))
        {
            ancestors.pop();
        }
        if let Some((parent_index, _)) = ancestors.last() {
            out[*parent_index] = out[*parent_index].saturating_sub(items[child_index].size);
        }
        ancestors.push((child_index, child_path));
    }

    out
}

/// Bytes an actual selection is expected to remove, with selected parent paths
/// covering descendants. Non-path resources are independent and are summed.
pub fn selection_size<'a>(items: impl IntoIterator<Item = &'a Candidate>) -> u64 {
    let selected: Vec<&Candidate> = items.into_iter().collect();
    let mut total = selected
        .iter()
        .filter(|item| item.footprint.is_none())
        .map(|item| item.size)
        .sum::<u64>();
    let mut paths: Vec<_> = selected
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            item.footprint
                .as_deref()
                .map(|path| (index, path, item.size))
        })
        .collect();
    paths.sort_unstable_by(|(left_index, left_path, _), (right_index, right_path, _)| {
        left_path.cmp(right_path).then(left_index.cmp(right_index))
    });

    // Lexical path order keeps every descendant directly after its selected
    // top-most owner. Remembering that owner is enough to skip the entire
    // covered subtree, including duplicate rows, in one pass.
    let mut owner: Option<&std::path::Path> = None;
    for (_, path, size) in paths {
        if owner.is_some_and(|owner| path == owner || path.starts_with(owner)) {
            continue;
        }
        total = total.saturating_add(size);
        owner = Some(path);
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Action, Category, Risk};
    use std::path::PathBuf;

    fn path(label: &str, path: &str, size: u64) -> Candidate {
        Candidate::new(
            Category::Artifacts,
            "test",
            label,
            "",
            size,
            Risk::Safe,
            Action::Remove(PathBuf::from(path)),
        )
    }

    #[test]
    fn nested_rows_partition_the_parent_instead_of_double_counting() {
        let items = vec![
            path("worktree", "/repo/worktree", 100),
            path("modules", "/repo/worktree/node_modules", 60),
            path("dist", "/repo/worktree/dist", 10),
        ];
        assert_eq!(exclusive_sizes(&items), [30, 60, 10]);
    }

    #[test]
    fn three_levels_assign_each_byte_to_the_most_specific_row_once() {
        let items = vec![
            path("worktree", "/repo/worktree", 100),
            path("modules", "/repo/worktree/node_modules", 70),
            path("cache", "/repo/worktree/node_modules/.cache", 20),
        ];
        assert_eq!(exclusive_sizes(&items), [30, 50, 20]);
    }

    #[test]
    fn duplicate_footprints_keep_the_first_owner_and_still_partition_children() {
        let items = vec![
            path("duplicate", "/repo", 100),
            path("owner", "/repo", 100),
            path("child", "/repo/build", 40),
        ];
        assert_eq!(exclusive_sizes(&items), [60, 0, 40]);
    }

    #[test]
    fn selecting_a_parent_and_child_promises_the_parent_once() {
        let mut items = vec![
            path("worktree", "/repo/worktree", 100),
            path("duplicate", "/repo/worktree", 70),
            path("modules", "/repo/worktree/node_modules", 60),
            path("sibling", "/repo/other", 20),
        ];
        for item in &mut items {
            item.selected = true;
        }
        assert_eq!(selection_size(items.iter()), 120);
    }
}
