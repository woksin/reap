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
    let paths: Vec<_> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| item.footprint.as_deref().map(|path| (index, path)))
        .collect();

    for (child_index, child_path) in &paths {
        // Exact duplicate footprints can arise when two rule sets recognise
        // the same directory. Keep the first as owner and do not assign the
        // duplicate to the common parent a second time.
        if paths
            .iter()
            .any(|(other_index, other_path)| other_index < child_index && other_path == child_path)
        {
            out[*child_index] = 0;
            continue;
        }

        // The deepest containing candidate is the immediate parent in the
        // finding hierarchy. Subtract the child's full measured size there;
        // its own descendants will in turn be partitioned from the child.
        let parent = paths
            .iter()
            .filter(|(parent_index, parent_path)| {
                parent_index != child_index
                    && parent_path != child_path
                    && child_path.starts_with(parent_path)
            })
            .max_by_key(|(_, parent_path)| parent_path.components().count());
        if let Some((parent_index, _)) = parent {
            out[*parent_index] = out[*parent_index].saturating_sub(items[*child_index].size);
        }
    }

    out
}

/// Bytes an actual selection is expected to remove, with selected parent paths
/// covering descendants. Non-path resources are independent and are summed.
pub fn selection_size<'a>(items: impl IntoIterator<Item = &'a Candidate>) -> u64 {
    let selected: Vec<&Candidate> = items.into_iter().collect();
    selected
        .iter()
        .enumerate()
        .filter(|(idx, item)| match item.footprint.as_deref() {
            Some(path) => !selected.iter().enumerate().any(|(other_idx, other)| {
                other.footprint.as_deref().is_some_and(|parent| {
                    (other_idx < *idx && parent == path)
                        || (other_idx != *idx && parent != path && path.starts_with(parent))
                })
            }),
            None => true,
        })
        .map(|(_, item)| item.size)
        .sum()
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
    fn selecting_a_parent_and_child_promises_the_parent_once() {
        let mut items = vec![
            path("worktree", "/repo/worktree", 100),
            path("modules", "/repo/worktree/node_modules", 60),
        ];
        for item in &mut items {
            item.selected = true;
        }
        assert_eq!(selection_size(items.iter()), 100);
    }
}
