//! Read-only disk catalogue.
//!
//! Reclaim scanners answer what can be removed. This scanner answers the
//! separate question that led someone to run reap in the first place: which
//! broad trees account for the occupied bytes? Rows from here never carry a
//! destructive action and are deliberately coarse; recognised reclaimable
//! descendants remain in their purpose-built categories.

use super::ScanOpts;
use crate::model::{Action, Candidate, Category, Eligibility, Risk, ScanEvent};
use crate::util::tilde;
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

const INVENTORY_FLOOR: u64 = 100_000_000;

pub fn scan(opts: &ScanOpts, tx: &Sender<ScanEvent>) {
    let _ = tx.send(ScanEvent::Status("cataloguing disk usage".into()));
    let mut rows: Vec<(String, PathBuf)> = Vec::new();

    if opts.roots.is_empty() {
        emit_notice(
            "No repository roots discovered",
            "add a root in Configuration or pass --path",
            opts,
            tx,
        );
    }
    for root in &opts.roots {
        if std::fs::read_dir(root).is_err() {
            emit_notice(
                &format!("Unreadable root: {}", tilde(root)),
                "repository and artifact coverage is incomplete",
                opts,
                tx,
            );
        }
    }

    if let Some(home) = super::home_dir() {
        collect_partition("home", &home, &opts.roots, &mut rows);
    }
    for mount in super::local_mount_roots() {
        collect_partition(
            &format!("volume {}", tilde(&mount)),
            &mount,
            &opts.roots,
            &mut rows,
        );
    }
    #[cfg(target_os = "macos")]
    for path in ["/Applications", "/Library", "/private/var", "/Users/Shared"] {
        let raw = PathBuf::from(path);
        if raw.is_dir() {
            let path = std::fs::canonicalize(&raw).unwrap_or(raw);
            if opts.roots.iter().any(|root| root.starts_with(&path)) {
                collect_partition("macOS data", &path, &opts.roots, &mut rows);
            } else {
                rows.push(("macOS data".to_string(), path));
            }
        }
    }
    for root in &opts.roots {
        collect_partition(&tilde(root), root, &[], &mut rows);
    }

    rows.sort_by(|a, b| a.1.cmp(&b.1));
    rows.dedup_by(|a, b| a.1 == b.1);

    let floor = opts.min_size.max(INVENTORY_FLOOR);
    rows.par_iter().for_each_with(tx.clone(), |tx, (group, path)| {
        let size = opts.cache.allocated_size(path);
        if size < floor {
            return;
        }
        let label = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let detail = if label == "com.docker.docker" {
            format!(
                "{} · Docker Desktop host allocation · logical images, volumes and cache are listed in Docker and must not be added twice",
                tilde(path)
            )
        } else {
            format!(
                "{} · host-allocated bytes, not a deletion candidate",
                tilde(path)
            )
        };
        let candidate = Candidate::new(
            Category::Storage,
            group.clone(),
            label,
            detail,
            size,
            Risk::Danger,
            Action::None,
        )
        .with_footprint(path.clone())
        .with_eligibility(Eligibility::Informational);
        super::emit(tx, opts, candidate);
    });
}

fn emit_notice(label: &str, detail: &str, opts: &ScanOpts, tx: &Sender<ScanEvent>) {
    let candidate = Candidate::new(
        Category::Storage,
        "coverage",
        label,
        detail,
        0,
        Risk::Danger,
        Action::None,
    )
    .with_eligibility(Eligibility::Protected);
    super::emit(tx, opts, candidate);
}

/// Emit a non-overlapping partition of `anchor`. If a configured scan root is a
/// direct child, use that root's children instead of also reporting its parent.
fn collect_partition(
    group: &str,
    anchor: &Path,
    roots: &[PathBuf],
    out: &mut Vec<(String, PathBuf)>,
) {
    let Ok(entries) = std::fs::read_dir(anchor) else {
        return;
    };
    let root_set: HashSet<&Path> = roots.iter().map(PathBuf::as_path).collect();
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let path = entry.path();
        if root_set.contains(path.as_path()) {
            collect_partition(&tilde(&path), &path, &[], out);
            continue;
        }
        if roots.iter().any(|root| root.starts_with(&path)) {
            // Partition the ancestors of a nested explicit root instead of
            // reporting the ancestor whole and the root again below it.
            collect_partition(group, &path, roots, out);
            continue;
        }

        // macOS concentrates most user-visible disk usage under Library. Split
        // only its broad application containers; the resulting rows remain a
        // partition, so Docker's host store is visible without also counting
        // the whole Library above it.
        if cfg!(target_os = "macos") && path.file_name().is_some_and(|name| name == "Library") {
            collect_macos_library(&path, out);
        } else {
            out.push((group.to_string(), path));
        }
    }
}

fn collect_macos_library(library: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(library) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let path = entry.path();
        let split = matches!(
            path.file_name().and_then(|name| name.to_str()),
            Some("Containers" | "Application Support" | "Caches")
        );
        if split {
            collect_partition(
                &format!(
                    "Library/{}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ),
                &path,
                &[],
                out,
            );
        } else {
            out.push(("Library".to_string(), path));
        }
    }
}
